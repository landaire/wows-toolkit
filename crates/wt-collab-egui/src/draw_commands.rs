//! Shared rendering of replay DrawCommands for both the desktop app and the web client.
//!
//! The desktop's `replay_renderer/shapes.rs` was the authoritative source; this module
//! is a direct port. All consumers (desktop + web) call into here.

use std::collections::HashMap;

use egui::Color32;
use egui::CornerRadius;
use egui::Pos2;
use egui::Rect;
use egui::Shape;
use egui::Stroke;
use egui::TextureHandle;
use egui::Vec2;

use wows_minimap_renderer::HUD_HEIGHT;
use wows_minimap_renderer::draw_command::DamageBreakdownEntry;
use wows_minimap_renderer::draw_command::DrawCommand;
use wt_translations::TextResolver;
use wt_translations::TranslatableText;

use crate::rendering::game_font;
use crate::rendering::make_rotated_icon_mesh;
use crate::transforms::MapTransform;

// ─── Public Types ────────────────────────────────────────────────────────────

/// Texture resources for DrawCommand rendering.
///
/// Desktop provides all fields; web may leave desktop-only sets as `None`.
pub struct DrawCommandTextures<'a> {
    pub ship_icons: &'a HashMap<String, TextureHandle>,
    /// Gold icon-shaped outlines for detected-teammate highlight (desktop only).
    pub ship_icon_outlines: Option<&'a HashMap<String, TextureHandle>>,
    pub plane_icons: &'a HashMap<String, TextureHandle>,
    pub building_icons: Option<&'a HashMap<String, TextureHandle>>,
    pub consumable_icons: Option<&'a HashMap<String, TextureHandle>>,
    pub death_cause_icons: Option<&'a HashMap<String, TextureHandle>>,
    pub powerup_icons: Option<&'a HashMap<String, TextureHandle>>,
    /// Ship silhouette for the stats panel HP overlay (desktop only).
    pub silhouette_texture: Option<&'a TextureHandle>,
}

/// Controls which labels are shown on ships and dead ships.
///
/// Desktop constructs this from `RenderOptions`; web uses `Default` (show all).
pub struct DrawCommandLabelOptions {
    pub show_player_names: bool,
    pub show_ship_names: bool,
    pub show_dead_ship_names: bool,
    /// When true, use `name_color` from Ship commands to tint ship-name labels.
    pub show_armament_color: bool,
}

impl Default for DrawCommandLabelOptions {
    fn default() -> Self {
        Self { show_player_names: true, show_ship_names: true, show_dead_ship_names: true, show_armament_color: false }
    }
}

// ─── Constants ───────────────────────────────────────────────────────────────

/// Default icon size in minimap-space pixels (matches `wows_minimap_renderer::assets::ICON_SIZE`).
const ICON_SIZE: f32 = (wows_minimap_renderer::MINIMAP_SIZE * 3 / 128) as f32;

// ─── Helpers ─────────────────────────────────────────────────────────────────

pub fn color_from_rgb(rgb: [u8; 3]) -> Color32 {
    Color32::from_rgb(rgb[0], rgb[1], rgb[2])
}

pub fn color_from_rgba(rgb: [u8; 3], alpha: f32) -> Color32 {
    Color32::from_rgba_unmultiplied(rgb[0], rgb[1], rgb[2], (alpha * 255.0) as u8)
}

/// Build an unrotated textured quad mesh for a plane/consumable icon.
fn make_icon_mesh(texture_id: egui::TextureId, center: Pos2, w: f32, h: f32) -> Shape {
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
///
/// `armament_color` is applied to `ship_name` if shown, otherwise `player_name`.
pub fn draw_ship_labels(
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

/// Map a `DeathCause` to the icon key used in the `death_cause_icons` HashMap.
fn death_cause_icon_key(cause: &wows_minimap_renderer::draw_command::KillFeedEntry) -> &'static str {
    use wows_replays::analyzer::decoder::DeathCause;
    match cause.cause.known() {
        Some(DeathCause::Artillery | DeathCause::ApShell | DeathCause::HeShell | DeathCause::CsShell) => "main_caliber",
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
    }
}

// ─── Core Rendering ──────────────────────────────────────────────────────────

/// Render a centered glow-text overlay (used by BattleResultOverlay and PreBattleCountdown).
fn render_glow_text_overlay(
    text: &str,
    subtitle: Option<&str>,
    color: &[u8; 3],
    subtitle_above: bool,
    transform: &MapTransform,
    ctx: &egui::Context,
) -> Vec<Shape> {
    let mut shapes = Vec::new();
    let ws = transform.window_scale;
    let canvas_w = transform.screen_hud_width();
    let canvas_h = (transform.hud_width + transform.hud_height) * transform.window_scale;
    let center_x = transform.origin.x + canvas_w / 2.0;
    let center_y = transform.origin.y + canvas_h / 2.0;

    let font_size = canvas_w / 8.0;
    let main_font = game_font(font_size);
    let main_galley = ctx.fonts_mut(|f| f.layout_no_wrap(text.to_string(), main_font, Color32::WHITE));
    let main_w = main_galley.size().x;
    let main_h = main_galley.size().y;

    let sub_galley = subtitle.map(|s| {
        let sub_font = game_font(font_size / 4.0);
        ctx.fonts_mut(|f| f.layout_no_wrap(s.to_string(), sub_font, Color32::from_gray(200)))
    });
    let sub_h = sub_galley.as_ref().map(|g| g.size().y).unwrap_or(0.0);
    let gap = if subtitle.is_some() { 8.0 * ws } else { 0.0 };
    let total_h = main_h + gap + sub_h;

    let block_top = center_y - total_h / 2.0;
    let (text_x, text_y, sub_top) = if subtitle_above {
        (center_x - main_w / 2.0, block_top + sub_h + gap, block_top)
    } else {
        (center_x - main_w / 2.0, block_top, block_top + main_h + gap)
    };

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
            let galley = ctx.fonts_mut(|f| f.layout_no_wrap(text.to_string(), glow_font.clone(), layer_color));
            shapes.push(Shape::galley(Pos2::new(text_x + dx * dist, text_y + dy * dist), galley, Color32::TRANSPARENT));
        }
    }

    shapes.push(Shape::galley(Pos2::new(text_x, text_y), main_galley, Color32::TRANSPARENT));

    if let Some(sub_galley) = sub_galley {
        let sub_w = sub_galley.size().x;
        let sub_x = center_x - sub_w / 2.0;
        let sub_y = sub_top;

        let sub_font = game_font(font_size / 4.0);
        for &(dx, dy) in offsets {
            let outline = ctx.fonts_mut(|f| {
                f.layout_no_wrap(
                    subtitle.unwrap().to_string(),
                    sub_font.clone(),
                    Color32::from_rgba_premultiplied(0, 0, 0, 180),
                )
            });
            shapes.push(Shape::galley(Pos2::new(sub_x + dx * 2.0, sub_y + dy * 2.0), outline, Color32::TRANSPARENT));
        }

        shapes.push(Shape::galley(Pos2::new(sub_x, sub_y), sub_galley, Color32::TRANSPARENT));
    }

    shapes
}

/// Convert a single DrawCommand into epaint shapes.
///
/// `placed_labels` is used by `ShipConfigCircle` to avoid label overlap. Pass `None`
/// if you don't need label collision detection (web client).
#[allow(clippy::too_many_arguments)]
pub fn draw_command_to_shapes(
    cmd: &DrawCommand,
    transform: &MapTransform,
    textures: &DrawCommandTextures,
    ctx: &egui::Context,
    label_opts: &DrawCommandLabelOptions,
    placed_labels: Option<&mut Vec<Rect>>,
    text_resolver: &dyn TextResolver,
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

            {
                let fallback_key = match (*visibility, *is_self) {
                    (wows_minimap_renderer::ShipVisibility::Visible, true) => "Auxiliary_self",
                    (wows_minimap_renderer::ShipVisibility::Visible, false) => "Auxiliary",
                    (
                        wows_minimap_renderer::ShipVisibility::MinimapOnly
                        | wows_minimap_renderer::ShipVisibility::Undetected,
                        _,
                    ) => "Auxiliary_invisible",
                };

                let (variant_key, texture) = if let Some(sp) = species {
                    let variant_key = match (*visibility, *is_self) {
                        (wows_minimap_renderer::ShipVisibility::Visible, true) => format!("{}_self", sp),
                        (wows_minimap_renderer::ShipVisibility::Visible, false) => sp.clone(),
                        (
                            wows_minimap_renderer::ShipVisibility::MinimapOnly
                            | wows_minimap_renderer::ShipVisibility::Undetected,
                            _,
                        ) => {
                            format!("{}_invisible", sp)
                        }
                    };
                    let tex = textures
                        .ship_icons
                        .get(&variant_key)
                        .or_else(|| textures.ship_icons.get(sp))
                        .or_else(|| textures.ship_icons.get(fallback_key));
                    (Some(variant_key), tex)
                } else {
                    (None, textures.ship_icons.get(fallback_key))
                };

                // Gold icon-shaped outline for detected teammates (drawn before icon)
                if *is_detected_teammate && let Some(outlines) = textures.ship_icon_outlines {
                    let outline_tex = variant_key
                        .as_ref()
                        .and_then(|vk| outlines.get(vk))
                        .or_else(|| species.as_ref().and_then(|sp| outlines.get(sp)));
                    if let Some(otex) = outline_tex {
                        shapes.push(make_rotated_icon_mesh(otex.id(), center, icon_size, *yaw, Color32::WHITE));
                    }
                }

                if let Some(tex) = texture {
                    let tint = if let Some(c) = color {
                        Color32::from_rgba_unmultiplied(c[0], c[1], c[2], (*opacity * 255.0) as u8)
                    } else {
                        Color32::from_rgba_unmultiplied(255, 255, 255, (*opacity * 255.0) as u8)
                    };
                    shapes.push(make_rotated_icon_mesh(tex.id(), center, icon_size, *yaw, tint));
                }
            }
            let pname = if label_opts.show_player_names { player_name.as_deref() } else { None };
            let sname = if label_opts.show_ship_names { ship_name.as_deref() } else { None };
            let arm_color = if label_opts.show_armament_color {
                name_color.map(|c| Color32::from_rgb(c[0], c[1], c[2]))
            } else {
                None
            };
            draw_ship_labels(ctx, center, transform.scale_distance(1.0), pname, sname, arm_color, &mut shapes);
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
            {
                let fallback_key = if *is_self { "Auxiliary_dead_self" } else { "Auxiliary_dead" };
                let variant_key = species
                    .as_ref()
                    .map(|sp| if *is_self { format!("{}_dead_self", sp) } else { format!("{}_dead", sp) });

                let texture = variant_key
                    .as_ref()
                    .and_then(|vk| textures.ship_icons.get(vk))
                    .or_else(|| species.as_ref().and_then(|sp| textures.ship_icons.get(sp)))
                    .or_else(|| textures.ship_icons.get(fallback_key));

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
            if label_opts.show_dead_ship_names {
                let pname = if label_opts.show_player_names { player_name.as_deref() } else { None };
                let sname = if label_opts.show_ship_names { ship_name.as_deref() } else { None };
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
            let pname = if label_opts.show_player_names { player_name.as_deref() } else { None };
            let sname = if label_opts.show_ship_names { ship_name.as_deref() } else { None };
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
            advantage,
        } => {
            let advantage_label = advantage
                .map(|(level, _)| text_resolver.resolve(&TranslatableText::Advantage(level)))
                .unwrap_or_default();
            let advantage_team = advantage.map(|(_, team)| team as i32).unwrap_or(-1);
            let canvas_w = transform.screen_hud_width();
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

            let t0_adv_w = if advantage_team == 0 && !advantage_label.is_empty() {
                let g = ctx.fonts_mut(|f| f.layout_no_wrap(advantage_label.clone(), adv_font.clone(), Color32::WHITE));
                let w = g.size().x;
                drop(g);
                Some(w)
            } else {
                None
            };

            let mut t0_total_w = t0_score_w;
            if let Some(tw) = t0_timer_w {
                t0_total_w += 4.0 * ws + tw;
            }
            if let Some(aw) = t0_adv_w {
                t0_total_w += 6.0 * ws + aw;
            }

            let pill_h = t0_score_h + pill_pad_y * 2.0;
            let pill_y = bar_origin.y + (bar_height - pill_h) / 2.0;

            let t0_pill_x = bar_origin.x + 8.0 * ws - pill_pad_x;
            shapes.push(Shape::rect_filled(
                Rect::from_min_size(Pos2::new(t0_pill_x, pill_y), Vec2::new(t0_total_w + pill_pad_x * 2.0, pill_h)),
                pill_rounding,
                pill_color,
            ));

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

            if t0_adv_w.is_some() {
                t0_cursor += 6.0 * ws;
                let ag = ctx.fonts_mut(|f| f.layout_no_wrap(advantage_label.clone(), adv_font.clone(), Color32::WHITE));
                let ay = score_top + (t0_score_h - ag.size().y) / 2.0;
                shapes.push(Shape::galley(Pos2::new(t0_cursor, ay), ag, Color32::WHITE));
            }

            // ── Measure all team 1 elements ──
            let t1_score_g = ctx.fonts_mut(|f| f.layout_no_wrap(t1_text.clone(), score_font.clone(), Color32::WHITE));
            let t1_score_w = t1_score_g.size().x;
            let t1_score_h = t1_score_g.size().y;
            drop(t1_score_g);

            let t1_timer_w = team1_timer.as_ref().map(|t| {
                let g = ctx.fonts_mut(|f| f.layout_no_wrap(t.clone(), timer_font.clone(), Color32::WHITE));
                let w = g.size().x;
                drop(g);
                w
            });

            let t1_adv_w = if advantage_team == 1 && !advantage_label.is_empty() {
                let g = ctx.fonts_mut(|f| f.layout_no_wrap(advantage_label.clone(), adv_font.clone(), Color32::WHITE));
                let w = g.size().x;
                drop(g);
                Some(w)
            } else {
                None
            };

            let mut t1_total_w = t1_score_w;
            if let Some(tw) = t1_timer_w {
                t1_total_w += 4.0 * ws + tw;
            }
            if let Some(aw) = t1_adv_w {
                t1_total_w += 6.0 * ws + aw;
            }

            let t1_pill_x = bar_origin.x + canvas_w - 8.0 * ws - t1_total_w - pill_pad_x;
            shapes.push(Shape::rect_filled(
                Rect::from_min_size(Pos2::new(t1_pill_x, pill_y), Vec2::new(t1_total_w + pill_pad_x * 2.0, pill_h)),
                pill_rounding,
                pill_color,
            ));

            let mut t1_cursor = bar_origin.x + canvas_w - 8.0 * ws;

            t1_cursor -= t1_score_w;
            let t1_score_galley = ctx.fonts_mut(|f| f.layout_no_wrap(t1_text, score_font, Color32::WHITE));
            let t1_score_top = pill_cy - t1_score_galley.size().y / 2.0;
            shapes.push(Shape::galley(Pos2::new(t1_cursor, t1_score_top), t1_score_galley, Color32::WHITE));

            if let Some(timer) = team1_timer {
                let tg = ctx.fonts_mut(|f| f.layout_no_wrap(timer.clone(), timer_font, timer_color));
                let tw = tg.size().x;
                t1_cursor -= 4.0 * ws + tw;
                let ty = t1_score_top + (t1_score_h - tg.size().y) / 2.0;
                shapes.push(Shape::galley(Pos2::new(t1_cursor, ty), tg, timer_color));
            }

            if let Some(aw) = t1_adv_w {
                t1_cursor -= 6.0 * ws + aw;
                let ag = ctx.fonts_mut(|f| f.layout_no_wrap(advantage_label.clone(), adv_font, Color32::WHITE));
                let ay = t1_score_top + (t1_score_h - ag.size().y) / 2.0;
                shapes.push(Shape::galley(Pos2::new(t1_cursor, ay), ag, Color32::WHITE));
            }
        }

        DrawCommand::Timer { time_remaining, elapsed } => {
            if elapsed.seconds() <= 0.0 {
                return shapes;
            }
            let canvas_w = transform.screen_hud_width();
            let main_font = game_font(16.0 * ws);
            let pill_color = Color32::from_rgba_unmultiplied(0, 0, 0, 140);
            let pill_pad_x = 4.0 * ws;
            let pill_pad_y = 1.0 * ws;
            let pill_rounding = CornerRadius::same((3.0 * ws) as u8);

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
            let text = format!("{}", seconds);
            let subtitle = text_resolver.resolve(&TranslatableText::PreBattleLabel);
            let color: [u8; 3] = [255, 200, 50];
            let subtitle_above = true;
            shapes.extend(render_glow_text_overlay(&text, Some(&subtitle), &color, subtitle_above, transform, ctx));
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
            let canvas_w = transform.screen_hud_width();
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

                let cause_key = death_cause_icon_key(entry);

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

                let has_cause_icon =
                    textures.death_cause_icons.as_ref().is_some_and(|icons| icons.contains_key(cause_key));
                let cause_w = if has_cause_icon { cause_icon_size } else { 0.0 };

                let has_killer_icon =
                    entry.killer_species.as_ref().is_some_and(|sp| textures.ship_icons.contains_key(sp.as_str()));
                let has_victim_icon =
                    entry.victim_species.as_ref().is_some_and(|sp| textures.ship_icons.contains_key(sp.as_str()));

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

                // If we don't have death cause icons, fall back to simple text-only layout
                if !has_cause_icon && !has_killer_icon && !has_victim_icon {
                    // Simple text kill feed: "killer ⚔ victim"
                    let text = format!("{} \u{2694} {}", entry.killer_name, entry.victim_name);
                    let galley = ctx.fonts_mut(|f| f.layout_no_wrap(text, name_font.clone(), Color32::WHITE));
                    let text_w = galley.size().x;
                    let x = start.x + canvas_w - right_margin - text_w;
                    let line_height = 14.0 * ws;
                    let pill_rect = Rect::from_min_size(
                        Pos2::new(x - 3.0 * ws, y - 1.0 * ws),
                        Vec2::new(text_w + 6.0 * ws, line_height),
                    );
                    shapes.push(Shape::rect_filled(
                        pill_rect,
                        CornerRadius::same((2.0 * ws) as u8),
                        Color32::from_rgba_unmultiplied(0, 0, 0, 140),
                    ));
                    shapes.push(Shape::galley(Pos2::new(x, y), galley, Color32::WHITE));
                    continue;
                }

                // Rich kill feed with icons
                let bg_x = start.x + canvas_w - total_w - right_margin * 2.0;
                let bg_rect =
                    Rect::from_min_size(Pos2::new(bg_x, y - 1.0 * ws), Vec2::new(total_w + right_margin * 2.0, line_h));
                shapes.push(Shape::rect_filled(bg_rect, CornerRadius::ZERO, Color32::from_black_alpha(128)));

                let mut x = start.x + canvas_w - total_w - right_margin;
                let row_rect = killer_galley.rows.first().map(|r| r.rect()).unwrap_or(egui::Rect::ZERO);
                let icon_center_y = y + row_rect.center().y;

                // Killer name
                shapes.push(Shape::galley(Pos2::new(x, y), killer_galley, Color32::TRANSPARENT));
                x += killer_name_w;

                // Killer ship icon (friendly=points left (PI), enemy=points right (0))
                if has_killer_icon {
                    x += gap;
                    let sp = entry.killer_species.as_ref().unwrap();
                    if let Some(tex) = textures.ship_icons.get(sp.as_str()) {
                        let tint =
                            Color32::from_rgb(entry.killer_color[0], entry.killer_color[1], entry.killer_color[2]);
                        let angle = if entry.killer_is_friendly { std::f32::consts::PI } else { 0.0 };
                        shapes.push(make_rotated_icon_mesh(
                            tex.id(),
                            Pos2::new(x + icon_size / 2.0, icon_center_y),
                            icon_size,
                            angle,
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
                if let Some(icons) = textures.death_cause_icons
                    && let Some(tex) = icons.get(cause_key)
                {
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

                // Victim ship icon (friendly=points left (PI), enemy=points right (0))
                if has_victim_icon {
                    x += gap;
                    let sp = entry.victim_species.as_ref().unwrap();
                    if let Some(tex) = textures.ship_icons.get(sp.as_str()) {
                        let tint =
                            Color32::from_rgb(entry.victim_color[0], entry.victim_color[1], entry.victim_color[2]);
                        let angle = if entry.victim_is_friendly { std::f32::consts::PI } else { 0.0 };
                        shapes.push(make_rotated_icon_mesh(
                            tex.id(),
                            Pos2::new(x + icon_size / 2.0, icon_center_y),
                            icon_size,
                            angle,
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

        DrawCommand::Building { pos, color, is_alive, icon_type, relation } => {
            let center = transform.minimap_to_screen(pos);
            let icon_key = icon_type.map(|t| format!("{}_{}", t.icon_name(), relation.icon_suffix()));
            let icon = icon_key.as_ref().and_then(|k| textures.building_icons.and_then(|icons| icons.get(k)));
            if let Some(tex) = icon {
                let size = transform.scale_distance(ICON_SIZE);
                shapes.push(make_icon_mesh(tex.id(), center, size, size));
            } else {
                let r = if *is_alive { transform.scale_distance(2.0) } else { transform.scale_distance(1.5) };
                shapes.push(Shape::circle_filled(center, r, color_from_rgb(*color)));
            }
        }

        DrawCommand::TurretDirection { pos, yaw, color, length, .. } => {
            let start = transform.minimap_to_screen(pos);
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
            if let Some(consumable_icons) = textures.consumable_icons {
                let center = transform.minimap_to_screen(pos);
                let base_offset = if *has_hp_bar { 26.0 } else { 23.0 };
                let icon_y = center.y + transform.scale_distance(base_offset);
                let icon_sz = transform.scale_distance(16.0);
                let gap = transform.scale_distance(1.0);
                let count = icon_keys.len() as f32;
                let total_width = count * icon_sz + (count - 1.0) * gap;
                let start_x = center.x - total_width / 2.0 + icon_sz / 2.0;
                for (i, icon_key) in icon_keys.iter().enumerate() {
                    let icon_x = start_x + i as f32 * (icon_sz + gap);
                    if let Some(tex) = consumable_icons.get(icon_key) {
                        let half = icon_sz / 2.0;
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

            // Label collision avoidance
            if let Some(text) = label {
                let galley = ctx.fonts_mut(|f| f.layout_no_wrap(text.clone(), game_font(10.0), circle_color));
                let text_w = galley.size().x;
                let text_h = galley.size().y;
                let label_gap = 4.0;

                let candidate_angles: [f32; 8] = [
                    -std::f32::consts::FRAC_PI_2,
                    -std::f32::consts::FRAC_PI_4,
                    0.0,
                    std::f32::consts::FRAC_PI_4,
                    std::f32::consts::FRAC_PI_2,
                    3.0 * std::f32::consts::FRAC_PI_4,
                    std::f32::consts::PI,
                    -3.0 * std::f32::consts::FRAC_PI_4,
                ];

                let compute_label_rect = |angle: f32| -> Rect {
                    let anchor_x = center.x + (screen_radius + label_gap) * angle.cos();
                    let anchor_y = center.y + (screen_radius + label_gap) * angle.sin();
                    let cos = angle.cos();
                    let sin = angle.sin();
                    let x = if cos < -0.3 {
                        anchor_x - text_w
                    } else if cos > 0.3 {
                        anchor_x
                    } else {
                        anchor_x - text_w / 2.0
                    };
                    let y = if sin < -0.3 {
                        anchor_y - text_h
                    } else if sin > 0.3 {
                        anchor_y
                    } else {
                        anchor_y - text_h / 2.0
                    };
                    Rect::from_min_size(Pos2::new(x, y), egui::vec2(text_w, text_h))
                };

                let mut best_rect = compute_label_rect(candidate_angles[0]);
                if let Some(ref labels) = placed_labels {
                    for &angle in &candidate_angles {
                        let rect = compute_label_rect(angle);
                        let overlaps = labels.iter().any(|prev| prev.intersects(rect));
                        if !overlaps {
                            best_rect = rect;
                            break;
                        }
                    }
                }

                if let Some(labels) = placed_labels {
                    labels.push(best_rect);
                }
                shapes.push(Shape::galley(best_rect.min, galley, Color32::TRANSPARENT));
            }
        }

        DrawCommand::BuffZone { pos, radius, color, alpha, marker_name } => {
            let center = transform.minimap_to_screen(pos);
            let r = transform.scale_distance(*radius as f32);

            shapes.push(Shape::circle_filled(center, r, color_from_rgba(*color, *alpha)));
            shapes.push(Shape::circle_stroke(
                center,
                r,
                Stroke::new(transform.scale_stroke(1.5), color_from_rgba(*color, 0.6)),
            ));

            if let Some(name) = marker_name
                && let Some(powerup_icons) = textures.powerup_icons
                && let Some(tex) = powerup_icons.get(name.as_str())
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

        DrawCommand::BattleResultOverlay { result, finish_type, color, subtitle_above } => {
            let text = text_resolver.resolve(&TranslatableText::BattleResult(*result));
            let subtitle = finish_type
                .as_ref()
                .map(|ft| text_resolver.resolve(&TranslatableText::FinishType(ft.clone())).to_uppercase());
            shapes.extend(render_glow_text_overlay(&text, subtitle.as_deref(), color, *subtitle_above, transform, ctx));
        }

        DrawCommand::TeamBuffs { friendly_buffs, enemy_buffs } => {
            if let Some(powerup_icons) = textures.powerup_icons {
                let canvas_w = transform.screen_hud_width();
                let icon_sz = 16.0 * ws;
                let gap = 2.0 * ws;
                let buff_y = transform.hud_pos(0.0, HUD_HEIGHT as f32).y;
                let origin_x = transform.hud_pos(0.0, 0.0).x;

                // Friendly buffs: left side
                let mut x = origin_x + 4.0 * ws;
                for (marker, count) in friendly_buffs {
                    if let Some(tex) = powerup_icons.get(marker.as_str()) {
                        let mut mesh = egui::Mesh::with_texture(tex.id());
                        let rect = Rect::from_min_size(Pos2::new(x, buff_y), Vec2::new(icon_sz, icon_sz));
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
                                Pos2::new(x + icon_sz, buff_y + 4.0 * ws),
                                galley,
                                Color32::TRANSPARENT,
                            ));
                            x += icon_sz + tw + gap;
                        } else {
                            x += icon_sz + gap;
                        }
                    }
                }

                // Enemy buffs: right side
                let mut x = origin_x + canvas_w - 4.0 * ws;
                for (marker, count) in enemy_buffs {
                    if let Some(tex) = powerup_icons.get(marker.as_str()) {
                        if *count > 1 {
                            let label = format!("{}", count);
                            let font = game_font(10.0 * ws);
                            let galley = ctx.fonts_mut(|f| f.layout_no_wrap(label, font, Color32::WHITE));
                            let tw = galley.size().x;
                            x -= tw;
                            shapes.push(Shape::galley(Pos2::new(x, buff_y + 4.0 * ws), galley, Color32::TRANSPARENT));
                            x -= icon_sz;
                        } else {
                            x -= icon_sz;
                        }

                        let mut mesh = egui::Mesh::with_texture(tex.id());
                        let rect = Rect::from_min_size(Pos2::new(x, buff_y), Vec2::new(icon_sz, icon_sz));
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
        }

        DrawCommand::ChatOverlay { entries } => {
            let canvas_w = transform.screen_hud_width();
            let canvas_h = (transform.hud_width + transform.hud_height) * transform.window_scale;
            let header_font = game_font(11.0 * ws);
            let msg_font = game_font(11.0 * ws);
            let line_h = 14.0 * ws;
            let icon_sz = 12.0 * ws;
            let padding = 6.0 * ws;
            let entry_gap = 6.0 * ws;

            let box_w = canvas_w * 0.25;
            let box_x = transform.origin.x + 4.0 * ws;
            let inner_w = box_w - padding * 2.0;

            struct ChatLayout {
                clan_galley: Option<std::sync::Arc<egui::Galley>>,
                name_galley: std::sync::Arc<egui::Galley>,
                ship_icon_species: Option<String>,
                ship_name_galley: Option<std::sync::Arc<egui::Galley>>,
                msg_galleys: Vec<std::sync::Arc<egui::Galley>>,
                opacity: f32,
                team_color: [u8; 3],
            }

            let mut layouts = Vec::new();
            let mut total_h = padding;
            for entry in entries {
                let opacity = entry.opacity;
                let alpha = (opacity * 255.0) as u8;
                let team_color = entry.team_color;
                let team_c = Color32::from_rgba_unmultiplied(team_color[0], team_color[1], team_color[2], alpha);

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

                let ship_name_galley = entry
                    .ship_name
                    .as_ref()
                    .map(|sn| ctx.fonts_mut(|f| f.layout_no_wrap(sn.clone(), header_font.clone(), team_c)));
                let has_ship_line = ship_name_galley.is_some();

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

            if !layouts.is_empty() {
                total_h += padding;
                let box_y = transform.origin.y + canvas_h / 2.0 - total_h / 2.0;

                let bg_rect = Rect::from_min_size(Pos2::new(box_x, box_y), Vec2::new(box_w, total_h));
                shapes.push(Shape::rect_filled(bg_rect, CornerRadius::same(3), Color32::from_black_alpha(90)));

                let mut y = box_y + padding;
                for layout in &layouts {
                    let alpha = (layout.opacity * 255.0) as u8;
                    let x = box_x + padding;

                    let mut nx = x;
                    if let Some(ref cg) = layout.clan_galley {
                        shapes.push(Shape::galley(Pos2::new(nx, y), cg.clone(), Color32::TRANSPARENT));
                        nx += cg.size().x;
                    }
                    shapes.push(Shape::galley(Pos2::new(nx, y), layout.name_galley.clone(), Color32::TRANSPARENT));
                    y += line_h;

                    if let Some(ref sng) = layout.ship_name_galley {
                        let mut sx = x;
                        if let Some(ref species) = layout.ship_icon_species {
                            if let Some(tex) = textures.ship_icons.get(species.as_str()) {
                                let tc = layout.team_color;
                                let tint = Color32::from_rgba_unmultiplied(tc[0], tc[1], tc[2], alpha);
                                let icon_center_y = y + sng.size().y / 2.0;
                                shapes.push(make_rotated_icon_mesh(
                                    tex.id(),
                                    Pos2::new(sx + icon_sz / 2.0, icon_center_y),
                                    icon_sz,
                                    0.0,
                                    tint,
                                ));
                            }
                            sx += icon_sz + 2.0 * ws;
                        }
                        shapes.push(Shape::galley(Pos2::new(sx, y), sng.clone(), Color32::TRANSPARENT));
                        y += line_h;
                    }

                    for galley in &layout.msg_galleys {
                        shapes.push(Shape::galley(Pos2::new(x, y), galley.clone(), Color32::TRANSPARENT));
                        y += galley.rows.len().max(1) as f32 * line_h;
                    }

                    y += entry_gap;
                }
            }
        }
        DrawCommand::StatsPanel { x, width } => {
            let origin = transform.hud_pos(*x as f32, 0.0);
            let size = Vec2::new(
                *width as f32 * ws,
                transform.hud_pos(0.0, wows_minimap_renderer::CANVAS_HEIGHT as f32).y - origin.y,
            );
            // Dark panel background
            shapes.push(Shape::rect_filled(
                Rect::from_min_size(origin, size),
                CornerRadius::ZERO,
                Color32::from_rgba_unmultiplied(30, 34, 42, 245),
            ));
            // Left border line
            shapes.push(Shape::LineSegment {
                points: [origin, Pos2::new(origin.x, origin.y + size.y)],
                stroke: Stroke::new(ws, Color32::from_rgba_unmultiplied(55, 60, 72, 200)),
            });
        }

        DrawCommand::StatsSilhouette {
            x, y, width, height, ship_param_id: _, hp_fraction, hp_current, hp_max, ..
        } => {
            let padding = 8.0 * ws;
            let origin = transform.hud_pos(*x as f32, *y as f32);
            let inner_x = origin.x + padding;
            let inner_w = *width as f32 * ws - padding * 2.0;
            let sil_area_h = *height as f32 * ws - 20.0 * ws; // leave room for HP text below

            // Draw silhouette with HP overlay if texture is available
            if let Some(sil_tex) = textures.silhouette_texture {
                let tex_size = sil_tex.size_vec2();
                let aspect = tex_size.x / tex_size.y;
                // Fit silhouette into available area preserving aspect ratio
                let fit_w = inner_w.min(sil_area_h * aspect);
                let fit_h = fit_w / aspect;
                let sil_x = inner_x + (inner_w - fit_w) / 2.0;
                let sil_y = origin.y + (sil_area_h - fit_h) / 2.0;
                let sil_rect = Rect::from_min_size(Pos2::new(sil_x, sil_y), Vec2::new(fit_w, fit_h));

                // Gray silhouette (base — represents missing HP)
                let mut gray_mesh = egui::Mesh::with_texture(sil_tex.id());
                gray_mesh.add_rect_with_uv(
                    sil_rect,
                    Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0)),
                    Color32::from_rgb(200, 200, 200),
                );
                shapes.push(Shape::Mesh(gray_mesh.into()));

                // HP-colored overlay clipped to hp_fraction from the left
                let hp_color = hp_bar_color_egui(*hp_fraction);
                let fill_w = fit_w * hp_fraction;
                if fill_w > 0.0 {
                    let clip_rect = Rect::from_min_size(Pos2::new(sil_x, sil_y), Vec2::new(fill_w, fit_h));
                    let mut hp_mesh = egui::Mesh::with_texture(sil_tex.id());
                    hp_mesh.add_rect_with_uv(
                        clip_rect,
                        Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(*hp_fraction, 1.0)),
                        hp_color,
                    );
                    shapes.push(Shape::Mesh(hp_mesh.into()));
                }
            } else {
                // Fallback: simple HP bar when no silhouette texture
                let bar_y = origin.y + 10.0 * ws;
                let bar_h = sil_area_h - 10.0 * ws;
                if bar_h > 0.0 {
                    shapes.push(Shape::rect_filled(
                        Rect::from_min_size(Pos2::new(inner_x, bar_y), Vec2::new(inner_w, bar_h)),
                        CornerRadius::same(2),
                        Color32::from_rgb(40, 40, 40),
                    ));
                    let hp_color = hp_bar_color_egui(*hp_fraction);
                    let fill_w = inner_w * hp_fraction;
                    if fill_w > 0.0 {
                        shapes.push(Shape::rect_filled(
                            Rect::from_min_size(Pos2::new(inner_x, bar_y), Vec2::new(fill_w, bar_h)),
                            CornerRadius::same(2),
                            hp_color,
                        ));
                    }
                }
            }

            // HP text: "12,345 / 42,750"
            let hp_text =
                format!("{} / {}", format_number_egui(*hp_current as i64), format_number_egui(*hp_max as i64));
            let hp_font = game_font(16.0 * ws);
            let hp_galley = ctx.fonts_mut(|f| f.layout_no_wrap(hp_text, hp_font, Color32::from_rgb(220, 220, 220)));
            let hp_x = inner_x + (inner_w - hp_galley.size().x) / 2.0;
            let hp_y = origin.y + sil_area_h;
            shapes.push(Shape::galley(Pos2::new(hp_x, hp_y), hp_galley, Color32::TRANSPARENT));
        }

        DrawCommand::StatsDamage {
            x,
            y,
            width,
            breakdowns,
            damage_spotting,
            spotting_breakdowns,
            damage_potential,
            potential_breakdowns,
        } => {
            let padding = 8.0 * ws;
            let origin = transform.hud_pos(*x as f32, *y as f32);
            let inner_x = origin.x + padding;
            let indent_x = inner_x + 12.0 * ws;
            let right_x = origin.x + *width as f32 * ws - padding;

            let header_font = game_font(16.0 * ws);
            let breakdown_font = game_font(13.0 * ws);
            let header_row_h = 22.0 * ws;
            let breakdown_row_h = 18.0 * ws;
            let label_color = Color32::from_rgb(140, 140, 140);

            let mut cur_y = origin.y + 4.0 * ws;

            // Total enemy damage header
            let total_damage: f64 = breakdowns.iter().map(|e| e.damage).sum();
            let header_galley = ctx.fonts_mut(|f| {
                f.layout_no_wrap("DMG".to_string(), header_font.clone(), Color32::from_rgb(200, 200, 200))
            });
            shapes.push(Shape::galley(Pos2::new(inner_x, cur_y), header_galley, Color32::TRANSPARENT));
            let total_str = format_number_egui(total_damage as i64);
            let total_galley =
                ctx.fonts_mut(|f| f.layout_no_wrap(total_str, header_font.clone(), Color32::from_rgb(255, 220, 100)));
            let total_x = right_x - total_galley.size().x;
            shapes.push(Shape::galley(Pos2::new(total_x, cur_y), total_galley, Color32::TRANSPARENT));
            cur_y += header_row_h;

            // Indented breakdown rows (smaller font)
            for entry in breakdowns.iter() {
                let color = damage_label_color(&entry.label);
                let label_galley =
                    ctx.fonts_mut(|f| f.layout_no_wrap(entry.label.clone(), breakdown_font.clone(), label_color));
                shapes.push(Shape::galley(Pos2::new(indent_x, cur_y), label_galley, Color32::TRANSPARENT));

                let val_str = format_number_egui(entry.damage as i64);
                let val_galley = ctx.fonts_mut(|f| f.layout_no_wrap(val_str, breakdown_font.clone(), color));
                let val_x = right_x - val_galley.size().x;
                shapes.push(Shape::galley(Pos2::new(val_x, cur_y), val_galley, Color32::TRANSPARENT));
                cur_y += breakdown_row_h;
            }

            // Separator before spot/potential
            if !breakdowns.is_empty() {
                let sep_y = cur_y - 1.0 * ws;
                shapes.push(Shape::LineSegment {
                    points: [Pos2::new(inner_x, sep_y), Pos2::new(right_x, sep_y)],
                    stroke: Stroke::new(ws * 0.5, Color32::from_rgba_unmultiplied(60, 60, 60, 150)),
                });
                cur_y += 2.0 * ws;
            }

            // Spotting + Potential: header row with total, then indented sub-breakdowns
            let summary_sections: [(&str, f64, &[DamageBreakdownEntry], Color32); 2] = [
                ("SPOT", *damage_spotting, spotting_breakdowns, Color32::from_rgb(120, 200, 255)),
                ("POT", *damage_potential, potential_breakdowns, Color32::from_rgb(180, 180, 180)),
            ];
            for (label, total, sub_breakdowns, color) in &summary_sections {
                // Header row
                let label_galley =
                    ctx.fonts_mut(|f| f.layout_no_wrap(label.to_string(), breakdown_font.clone(), label_color));
                shapes.push(Shape::galley(Pos2::new(inner_x, cur_y), label_galley, Color32::TRANSPARENT));

                let val_str = format_number_egui(*total as i64);
                let val_galley = ctx.fonts_mut(|f| f.layout_no_wrap(val_str, breakdown_font.clone(), *color));
                let val_x = right_x - val_galley.size().x;
                shapes.push(Shape::galley(Pos2::new(val_x, cur_y), val_galley, Color32::TRANSPARENT));
                cur_y += breakdown_row_h;

                // Sub-breakdown rows (indented, dimmer)
                for entry in sub_breakdowns.iter() {
                    let sub_color = damage_label_color(&entry.label);
                    let sub_label =
                        ctx.fonts_mut(|f| f.layout_no_wrap(entry.label.clone(), breakdown_font.clone(), label_color));
                    shapes.push(Shape::galley(Pos2::new(indent_x, cur_y), sub_label, Color32::TRANSPARENT));

                    let sub_val_str = format_number_egui(entry.damage as i64);
                    let sub_val = ctx.fonts_mut(|f| f.layout_no_wrap(sub_val_str, breakdown_font.clone(), sub_color));
                    let sub_val_x = right_x - sub_val.size().x;
                    shapes.push(Shape::galley(Pos2::new(sub_val_x, cur_y), sub_val, Color32::TRANSPARENT));
                    cur_y += breakdown_row_h;
                }
            }
        }

        DrawCommand::StatsRibbons { x, y, width, ribbons } => {
            let padding = 8.0 * ws;
            let origin = transform.hud_pos(*x as f32, *y as f32);
            let inner_x = origin.x + padding;
            let inner_w = (*width as f32 * ws - padding * 2.0) / 2.0;
            let row_h = 20.0 * ws;
            let font = game_font(14.0 * ws);
            let name_color = Color32::from_rgb(180, 180, 180);
            let count_color = Color32::from_rgb(255, 220, 100);

            for (i, rc) in ribbons.iter().take(12).enumerate() {
                let col = i % 2;
                let row = i / 2;
                let rx = inner_x + col as f32 * inner_w;
                let ry = origin.y + row as f32 * row_h;

                let name_galley =
                    ctx.fonts_mut(|f| f.layout_no_wrap(rc.display_name.clone(), font.clone(), name_color));
                shapes.push(Shape::galley(Pos2::new(rx, ry), name_galley, Color32::TRANSPARENT));

                let count_str = format!("x{}", rc.count);
                let count_galley = ctx.fonts_mut(|f| f.layout_no_wrap(count_str, font.clone(), count_color));
                let count_x = rx + inner_w - count_galley.size().x;
                shapes.push(Shape::galley(Pos2::new(count_x, ry), count_galley, Color32::TRANSPARENT));
            }
        }

        DrawCommand::StatsActivityFeed { x, y, width, height, entries } => {
            let padding = 8.0 * ws;
            let origin = transform.hud_pos(*x as f32, *y as f32);
            let inner_x = origin.x + padding;
            let inner_w = *width as f32 * ws - padding * 2.0;
            let total_h = *height as f32 * ws;
            let name_font = game_font(14.0 * ws);
            let msg_font = game_font(13.0 * ws);
            let icon_size = 16.0 * ws;
            let gap = 2.0 * ws;

            // Fixed-size box background
            let box_rect = Rect::from_min_size(origin, Vec2::new(*width as f32 * ws, total_h));
            shapes.push(Shape::rect_filled(
                box_rect,
                CornerRadius::same(2),
                Color32::from_rgba_unmultiplied(24, 28, 36, 200),
            ));
            // Top border
            shapes.push(Shape::LineSegment {
                points: [
                    Pos2::new(origin.x + 4.0 * ws, origin.y),
                    Pos2::new(origin.x + (*width as f32 - 4.0) * ws, origin.y),
                ],
                stroke: Stroke::new(ws * 0.5, Color32::from_rgba_unmultiplied(55, 60, 72, 150)),
            });

            // All feed content goes into a clipped group
            let mut feed_shapes: Vec<Shape> = Vec::new();

            // Pre-compute entry heights to show most recent that fit
            struct EntryLayout {
                height: f32,
            }
            let kill_row_h = 20.0 * ws;
            let chat_header_h = 18.0 * ws;
            let chat_msg_h = 17.0 * ws;

            let mut layouts: Vec<EntryLayout> = Vec::new();
            for entry in entries.iter() {
                let h = match &entry.kind {
                    wows_minimap_renderer::draw_command::ActivityFeedKind::Kill(_) => kill_row_h,
                    wows_minimap_renderer::draw_command::ActivityFeedKind::Chat(chat) => {
                        let msg_galley = ctx.fonts_mut(|f| {
                            let job = egui::text::LayoutJob::simple(
                                chat.message.clone(),
                                msg_font.clone(),
                                Color32::WHITE,
                                inner_w,
                            );
                            f.layout_job(job)
                        });
                        let lines = msg_galley.rows.len().max(1) as f32;
                        chat_header_h + lines * chat_msg_h + 2.0 * ws
                    }
                };
                layouts.push(EntryLayout { height: h });
            }

            // Show most recent entries that fit
            let mut consumed = 0.0f32;
            let mut start_idx = entries.len();
            for i in (0..entries.len()).rev() {
                let needed = consumed + layouts[i].height;
                if needed > total_h - 4.0 * ws {
                    break;
                }
                consumed = needed;
                start_idx = i;
            }

            let mut ey = origin.y + 4.0 * ws;
            for entry in entries.iter().skip(start_idx) {
                if ey >= origin.y + total_h {
                    break;
                }
                match &entry.kind {
                    wows_minimap_renderer::draw_command::ActivityFeedKind::Kill(kill) => {
                        let killer_color = color_from_rgb(kill.killer_color);
                        let victim_color = color_from_rgb(kill.victim_color);

                        let mut cx = inner_x;

                        // Kill prefix
                        let prefix_galley = ctx.fonts_mut(|f| {
                            f.layout_no_wrap(" | ".into(), name_font.clone(), Color32::from_rgb(140, 140, 140))
                        });
                        feed_shapes.push(Shape::galley(Pos2::new(cx, ey), prefix_galley.clone(), Color32::TRANSPARENT));
                        cx += prefix_galley.size().x;

                        // Killer name
                        let killer_galley = ctx
                            .fonts_mut(|f| f.layout_no_wrap(kill.killer_name.clone(), name_font.clone(), killer_color));
                        let row_center_y = ey + killer_galley.size().y / 2.0;
                        feed_shapes.push(Shape::galley(Pos2::new(cx, ey), killer_galley.clone(), Color32::TRANSPARENT));
                        cx += killer_galley.size().x + gap;

                        // Killer ship icon (friendly=points left (PI), enemy=points right (0))
                        if let Some(ref species) = kill.killer_species
                            && let Some(tex) = textures.ship_icons.get(species.as_str())
                        {
                            let tint =
                                Color32::from_rgb(kill.killer_color[0], kill.killer_color[1], kill.killer_color[2]);
                            let angle = if kill.killer_is_friendly { std::f32::consts::PI } else { 0.0 };
                            feed_shapes.push(crate::rendering::make_rotated_icon_mesh(
                                tex.id(),
                                Pos2::new(cx + icon_size / 2.0, row_center_y),
                                icon_size,
                                angle,
                                tint,
                            ));
                            cx += icon_size + gap;
                        }

                        // Death cause icon
                        let cause_key = death_cause_icon_key(kill);
                        if let Some(icons) = textures.death_cause_icons.as_ref()
                            && let Some(tex) = icons.get(cause_key)
                        {
                            let half = icon_size / 2.0;
                            let mut mesh = egui::Mesh::with_texture(tex.id());
                            mesh.add_rect_with_uv(
                                Rect::from_min_max(
                                    Pos2::new(cx, row_center_y - half),
                                    Pos2::new(cx + icon_size, row_center_y + half),
                                ),
                                Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0)),
                                Color32::WHITE,
                            );
                            feed_shapes.push(Shape::Mesh(mesh.into()));
                            cx += icon_size + gap;
                        }

                        // Victim name
                        let victim_galley = ctx
                            .fonts_mut(|f| f.layout_no_wrap(kill.victim_name.clone(), name_font.clone(), victim_color));
                        feed_shapes.push(Shape::galley(Pos2::new(cx, ey), victim_galley.clone(), Color32::TRANSPARENT));
                        cx += victim_galley.size().x + gap;

                        // Victim ship icon (friendly=points left (PI), enemy=points right (0))
                        if let Some(ref species) = kill.victim_species
                            && let Some(tex) = textures.ship_icons.get(species.as_str())
                        {
                            let tint =
                                Color32::from_rgb(kill.victim_color[0], kill.victim_color[1], kill.victim_color[2]);
                            let angle = if kill.victim_is_friendly { std::f32::consts::PI } else { 0.0 };
                            feed_shapes.push(crate::rendering::make_rotated_icon_mesh(
                                tex.id(),
                                Pos2::new(cx + icon_size / 2.0, row_center_y),
                                icon_size,
                                angle,
                                tint,
                            ));
                        }

                        ey += kill_row_h;
                    }
                    wows_minimap_renderer::draw_command::ActivityFeedKind::Chat(chat) => {
                        let team_c = color_from_rgb(chat.team_color);
                        let msg_c = color_from_rgb(chat.message_color);

                        let mut cx = inner_x;

                        // Clan tag
                        if !chat.clan_tag.is_empty() {
                            let clan_c = chat.clan_color.map_or(team_c, color_from_rgb);
                            let clan_galley = ctx.fonts_mut(|f| {
                                f.layout_no_wrap(format!("[{}] ", chat.clan_tag), name_font.clone(), clan_c)
                            });
                            feed_shapes.push(Shape::galley(
                                Pos2::new(cx, ey),
                                clan_galley.clone(),
                                Color32::TRANSPARENT,
                            ));
                            cx += clan_galley.size().x;
                        }

                        // Player name
                        let name_galley =
                            ctx.fonts_mut(|f| f.layout_no_wrap(chat.player_name.clone(), name_font.clone(), team_c));
                        let row_center_y = ey + name_galley.size().y / 2.0;
                        feed_shapes.push(Shape::galley(Pos2::new(cx, ey), name_galley.clone(), Color32::TRANSPARENT));
                        cx += name_galley.size().x + gap;

                        // Ship icon
                        if let Some(ref species) = chat.ship_species
                            && let Some(tex) = textures.ship_icons.get(species.as_str())
                        {
                            let tint = Color32::from_rgb(chat.team_color[0], chat.team_color[1], chat.team_color[2]);
                            feed_shapes.push(crate::rendering::make_rotated_icon_mesh(
                                tex.id(),
                                Pos2::new(cx + icon_size / 2.0, row_center_y),
                                icon_size,
                                0.0,
                                tint,
                            ));
                            cx += icon_size + gap;
                        }

                        // Ship name
                        if let Some(ref ship_name) = chat.ship_name {
                            let sn_galley =
                                ctx.fonts_mut(|f| f.layout_no_wrap(ship_name.clone(), name_font.clone(), team_c));
                            feed_shapes.push(Shape::galley(Pos2::new(cx, ey), sn_galley, Color32::TRANSPARENT));
                        }
                        ey += chat_header_h;

                        // Message body
                        let msg_galley = ctx.fonts_mut(|f| {
                            let job =
                                egui::text::LayoutJob::simple(chat.message.clone(), msg_font.clone(), msg_c, inner_w);
                            f.layout_job(job)
                        });
                        feed_shapes.push(Shape::galley(
                            Pos2::new(inner_x, ey),
                            msg_galley.clone(),
                            Color32::TRANSPARENT,
                        ));
                        ey += msg_galley.rows.len().max(1) as f32 * chat_msg_h + 2.0 * ws;
                    }
                }
            }

            // Add feed content to output shapes
            shapes.extend(feed_shapes);
        }
    }

    shapes
}

// ─── Stats panel helpers ──────────────────────────────────────────────────

fn format_number_egui(n: i64) -> String {
    if n < 0 {
        return format!("-{}", format_number_egui(-n));
    }
    let s = n.to_string();
    let mut result = String::with_capacity(s.len() + s.len() / 3);
    for (i, c) in s.chars().rev().enumerate() {
        if i > 0 && i % 3 == 0 {
            result.push(',');
        }
        result.push(c);
    }
    result.chars().rev().collect()
}

fn damage_label_color(label: &str) -> Color32 {
    match label {
        "AP" => Color32::from_rgb(255, 200, 80),
        "HE" => Color32::from_rgb(255, 140, 50),
        "SAP" => Color32::from_rgb(200, 180, 255),
        "MAIN" => Color32::from_rgb(255, 200, 80),
        "SEC" => Color32::from_rgb(255, 170, 60),
        "TORP" => Color32::from_rgb(100, 200, 255),
        "FIRE" => Color32::from_rgb(255, 120, 50),
        "FLOOD" => Color32::from_rgb(80, 160, 255),
        "BOMB" => Color32::from_rgb(220, 180, 100),
        "ROCKET" => Color32::from_rgb(230, 150, 80),
        "DC" => Color32::from_rgb(160, 200, 160),
        "RAM" => Color32::from_rgb(200, 100, 100),
        "MISSILE" => Color32::from_rgb(220, 130, 220),
        _ => Color32::from_rgb(180, 180, 180),
    }
}

fn hp_bar_color_egui(fraction: f32) -> Color32 {
    if fraction > 0.66 {
        Color32::from_rgb(0, 255, 0)
    } else if fraction > 0.33 {
        let t = (fraction - 0.33) / 0.33;
        Color32::from_rgb((255.0 * (1.0 - t)) as u8, 255, 0)
    } else {
        let t = fraction / 0.33;
        Color32::from_rgb(255, (255.0 * t) as u8, 0)
    }
}

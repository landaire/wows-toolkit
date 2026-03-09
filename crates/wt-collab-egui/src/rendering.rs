//! Shared annotation rendering, hit testing, and geometry helpers.
//!
//! These functions are used by both the desktop replay renderer / tactics board
//! and the WASM web client.

use std::collections::HashMap;

use egui::Color32;
use egui::FontId;
use egui::Pos2;
use egui::Rect;
use egui::Shape;
use egui::Stroke;
use egui::TextureHandle;
use egui::Vec2;

use crate::transforms::MapTransform;
use crate::types::Annotation;
use crate::types::CapPointView;
use crate::types::ENEMY_COLOR;
use crate::types::FRIENDLY_COLOR;
use crate::types::GridStyle;
use crate::types::MapPing;
use crate::types::PaintTool;
use crate::types::UserCursor;
use wows_minimap_renderer::MINIMAP_SIZE;
use wows_minimap_renderer::MapInfo;
use wows_minimap_renderer::MinimapPos;
use wows_minimap_renderer::map_data::WorldPos;

// ─── Constants ───────────────────────────────────────────────────────────────

/// Default icon size in minimap-space pixels (matches `wows_minimap_renderer::assets::ICON_SIZE`).
pub const ICON_SIZE_F32: f32 = (MINIMAP_SIZE * 3 / 128) as f32;

pub const ROTATION_HANDLE_RADIUS: f32 = 5.0;
pub const ROTATION_HANDLE_DISTANCE: f32 = 25.0;

/// Duration in seconds that a ping ripple is visible.
pub const PING_DURATION: f32 = 1.0;

/// BigWorld-to-meters conversion factor (1 BW unit = 30 meters).
const BW_TO_METERS: f32 = 30.0;

/// Font ID using the game font family (Warhelios Bold + CJK fallbacks).
pub fn game_font(size: f32) -> FontId {
    FontId::new(size, egui::FontFamily::Name("GameFont".into()))
}

// ─── Shared Range Circle Rendering ──────────────────────────────────────────

/// Visual style for a range circle kind: `(rgb, alpha, dashed)`.
pub fn range_circle_style(kind: RangeCircleKind) -> ([u8; 3], f32, bool) {
    match kind {
        RangeCircleKind::Detection => ([135, 206, 235], 0.6, true),
        RangeCircleKind::MainBattery => ([180, 180, 180], 0.5, false),
        RangeCircleKind::SecondaryBattery => ([255, 165, 0], 0.5, false),
        RangeCircleKind::TorpedoRange => ([0, 200, 200], 0.5, false),
        RangeCircleKind::Radar => ([255, 255, 100], 0.5, false),
        RangeCircleKind::Hydro => ([100, 255, 100], 0.5, false),
    }
}

/// Range circle kinds (mirrors [`wows_minimap_renderer::draw_command::ShipConfigCircleKind`]
/// without requiring the minimap-renderer dependency in all consumers).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RangeCircleKind {
    Detection,
    MainBattery,
    SecondaryBattery,
    TorpedoRange,
    Radar,
    Hydro,
}

/// Draw a range circle with optional label and collision-avoidance.
///
/// * `center` — screen-space center.
/// * `screen_radius` — radius in screen pixels.
/// * `color` — RGB color.
/// * `alpha` — opacity (0.0–1.0).
/// * `dashed` — if true, draw as a dashed circle.
/// * `label` — optional label text placed at the circle edge.
/// * `placed_labels` — optional mutable list for label collision avoidance.
pub fn draw_range_circle(
    ctx: &egui::Context,
    shapes: &mut Vec<Shape>,
    center: Pos2,
    screen_radius: f32,
    color: [u8; 3],
    alpha: f32,
    dashed: bool,
    label: Option<&str>,
    placed_labels: Option<&mut Vec<Rect>>,
) {
    let circle_color = Color32::from_rgba_unmultiplied(color[0], color[1], color[2], (alpha * 255.0) as u8);
    let stroke = Stroke::new(1.5, circle_color);

    if dashed {
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

    // Label with collision avoidance
    if let Some(text) = label {
        let galley = ctx.fonts_mut(|f| f.layout_no_wrap(text.to_owned(), game_font(10.0), circle_color));
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

/// Helper to convert a minimap `Vec2` position to screen `Pos2` via [`MapTransform`].
pub fn minimap_vec2_to_screen(pos: Vec2, transform: &MapTransform) -> Pos2 {
    transform.minimap_to_screen(&MinimapPos { x: pos.x as i32, y: pos.y as i32 })
}

// ─── Grid Overlay ───────────────────────────────────────────────────────────

/// Draw a 10×10 grid overlay with coordinate labels (1-10 columns, A-J rows).
pub fn draw_grid(painter: &egui::Painter, transform: &MapTransform, style: &GridStyle) {
    let cell = MINIMAP_SIZE as f32 / 10.0;
    let stroke_w = transform.scale_stroke(style.line_width);

    for i in 1..10 {
        let offset = (i as f32 * cell) as i32;
        let top = transform.minimap_to_screen(&MinimapPos { x: offset, y: 0 });
        let bottom = transform.minimap_to_screen(&MinimapPos { x: offset, y: MINIMAP_SIZE as i32 });
        painter.line_segment([top, bottom], Stroke::new(stroke_w, style.grid_color));

        let left = transform.minimap_to_screen(&MinimapPos { x: 0, y: offset });
        let right = transform.minimap_to_screen(&MinimapPos { x: MINIMAP_SIZE as i32, y: offset });
        painter.line_segment([left, right], Stroke::new(stroke_w, style.grid_color));
    }

    let font_size = transform.scale_stroke(style.label_font.size).max(7.0);
    let font = FontId::new(font_size, style.label_font.family.clone());

    for i in 0..10 {
        let x = (i as f32 + 0.5) * cell;
        let pos = transform.minimap_to_screen(&MinimapPos { x: x as i32, y: (cell * 0.15) as i32 });
        painter.text(pos, egui::Align2::CENTER_CENTER, format!("{}", i + 1), font.clone(), style.label_color);
    }

    for i in 0..10 {
        let y = (i as f32 + 0.5) * cell;
        let pos = transform.minimap_to_screen(&MinimapPos { x: (cell * 0.15) as i32, y: y as i32 });
        let label = (b'A' + i as u8) as char;
        painter.text(pos, egui::Align2::CENTER_CENTER, label.to_string(), font.clone(), style.label_color);
    }
}

// ─── Map Background ────────────────────────────────────────────────────────

/// Draw the map background texture, or a solid dark fallback if no texture is available.
///
/// The texture is drawn in the map region (below any HUD area), mapped to
/// the full [0..MINIMAP_SIZE] × [0..MINIMAP_SIZE] minimap space.
pub fn draw_map_background(painter: &egui::Painter, transform: &MapTransform, texture_id: Option<egui::TextureId>) {
    if let Some(tex_id) = texture_id {
        let map_tl = transform.minimap_to_screen(&MinimapPos { x: 0, y: 0 });
        let map_br = transform.minimap_to_screen(&MinimapPos { x: MINIMAP_SIZE as i32, y: MINIMAP_SIZE as i32 });
        let map_rect = Rect::from_min_max(map_tl, map_br);
        let uv = Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0));
        let mut mesh = egui::Mesh::with_texture(tex_id);
        mesh.add_rect_with_uv(map_rect, uv, Color32::WHITE);
        painter.add(Shape::Mesh(mesh.into()));
    } else {
        let map_tl = transform.minimap_to_screen(&MinimapPos { x: 0, y: 0 });
        let map_br = transform.minimap_to_screen(&MinimapPos { x: MINIMAP_SIZE as i32, y: MINIMAP_SIZE as i32 });
        painter.rect_filled(Rect::from_min_max(map_tl, map_br), 0.0, Color32::from_rgb(30, 40, 60));
    }
}

// ─── Capture Points ─────────────────────────────────────────────────────────

/// Alpha for cap point fill color (≈15% opacity).
const CAP_FILL_ALPHA: u8 = 38;

/// Neutral cap point fill (white at 15% opacity).
pub const CAP_NEUTRAL_FILL: Color32 = Color32::from_rgba_premultiplied(38, 38, 38, CAP_FILL_ALPHA);
/// Neutral cap point outline (white).
pub const CAP_NEUTRAL_OUTLINE: Color32 = Color32::from_rgb(255, 255, 255);

/// Team 0 (green/friendly) cap point fill.
pub const CAP_TEAM0_FILL: Color32 = Color32::from_rgba_premultiplied(11, 34, 25, CAP_FILL_ALPHA);
/// Team 0 (green/friendly) cap point outline.
pub const CAP_TEAM0_OUTLINE: Color32 = Color32::from_rgb(76, 232, 170);

/// Team 1 (red/enemy) cap point fill.
pub const CAP_TEAM1_FILL: Color32 = Color32::from_rgba_premultiplied(37, 11, 6, CAP_FILL_ALPHA);
/// Team 1 (red/enemy) cap point outline.
pub const CAP_TEAM1_OUTLINE: Color32 = Color32::from_rgb(254, 77, 42);

/// Return the fill and outline colors for a cap point based on team_id.
pub fn cap_point_colors(team_id: i64) -> (Color32, Color32) {
    match team_id {
        0 => (CAP_TEAM0_FILL, CAP_TEAM0_OUTLINE),
        1 => (CAP_TEAM1_FILL, CAP_TEAM1_OUTLINE),
        _ => (CAP_NEUTRAL_FILL, CAP_NEUTRAL_OUTLINE),
    }
}

/// Render a single capture point zone on the minimap (circle + label).
///
/// Draws the filled circle, team-colored outline, and letter label (A, B, C, …).
/// Does NOT draw selection handles — callers that need selection visuals should
/// draw them separately after calling this function.
pub fn render_cap_point(painter: &egui::Painter, transform: &MapTransform, map_info: &MapInfo, cap: &CapPointView) {
    let world_pos = WorldPos { x: cap.world_x, y: 0.0, z: cap.world_z };
    let minimap_pos = map_info.world_to_minimap(world_pos, MINIMAP_SIZE);
    let center = transform.minimap_to_screen(&minimap_pos);
    let radius_minimap = map_info.world_distance_to_minimap(cap.radius, MINIMAP_SIZE);
    let radius_screen = transform.scale_distance(radius_minimap);

    let (fill, outline) = cap_point_colors(cap.team_id);

    painter.circle_filled(center, radius_screen, fill);
    let outline_width = transform.scale_stroke(1.5);
    painter.circle_stroke(center, radius_screen, Stroke::new(outline_width, outline));

    // Label: A, B, C, …
    let letter = (b'A' + cap.index as u8) as char;
    painter.text(
        center,
        egui::Align2::CENTER_CENTER,
        letter.to_string(),
        game_font(11.0 * transform.window_scale),
        Color32::WHITE,
    );
}

// ─── Map Pings (click ripple) ───────────────────────────────────────────────

/// Draw active pings onto `painter` and return `true` if any were drawn
/// (so the caller can request a repaint). Expired pings are skipped.
pub fn draw_pings(pings: &[MapPing], painter: &egui::Painter, transform: &MapTransform) -> bool {
    let now = web_time::Instant::now();
    let mut any = false;

    for ping in pings {
        let age = now.duration_since(ping.time).as_secs_f32();
        if age >= PING_DURATION {
            continue;
        }
        any = true;
        let max_r = transform.scale_distance(40.0);
        let r = age * max_r;
        let alpha = ((1.0 - age / PING_DURATION) * 200.0) as u8;
        let [pr, pg, pb] = ping.color;
        let ping_color = Color32::from_rgba_unmultiplied(pr, pg, pb, alpha);
        let screen_pos = transform.minimap_to_screen(&MinimapPos { x: ping.pos[0] as i32, y: ping.pos[1] as i32 });
        painter.add(Shape::circle_stroke(screen_pos, r, Stroke::new(2.0, ping_color)));
        painter.add(Shape::circle_stroke(screen_pos, r * 0.6, Stroke::new(1.5, ping_color)));
    }
    any
}

// ─── Remote Cursors ─────────────────────────────────────────────────────────

/// Draw remote peer cursors with name labels. Cursors fade after 3s and disappear after 5s.
pub fn draw_remote_cursors(cursors: &[UserCursor], my_user_id: u64, painter: &egui::Painter, transform: &MapTransform) {
    let now = web_time::Instant::now();
    for cursor in cursors {
        if cursor.user_id == my_user_id {
            continue;
        }
        let Some(pos) = cursor.pos else { continue };
        let age = now.duration_since(cursor.last_update).as_secs_f32();
        if age > 5.0 {
            continue;
        }
        let alpha = if age > 3.0 { ((5.0 - age) / 2.0 * 255.0) as u8 } else { 255 };
        let [r, g, b] = cursor.color;
        let color = Color32::from_rgba_unmultiplied(r, g, b, alpha);

        let screen_pos = transform.minimap_to_screen(&MinimapPos { x: pos[0] as i32, y: pos[1] as i32 });

        let size = 10.0;
        let points =
            vec![screen_pos, screen_pos + Vec2::new(0.0, size * 1.5), screen_pos + Vec2::new(size * 0.6, size * 1.1)];
        painter.add(Shape::convex_polygon(
            points,
            color,
            Stroke::new(1.0, Color32::from_rgba_unmultiplied(0, 0, 0, alpha)),
        ));

        let label_pos = screen_pos + Vec2::new(size * 0.8, size * 0.5);
        let galley = painter.layout_no_wrap(cursor.name.clone(), FontId::proportional(11.0), color);
        let label_rect = Rect::from_min_size(label_pos - Vec2::new(2.0, 1.0), galley.size() + Vec2::new(4.0, 2.0));
        painter.rect_filled(label_rect, 2.0, Color32::from_rgba_unmultiplied(0, 0, 0, alpha / 2));
        painter.galley(label_pos, galley, Color32::PLACEHOLDER);
    }
}

// ─── Hit Testing ─────────────────────────────────────────────────────────────

/// Compute the distance from a point (in minimap coords) to the nearest part
/// of an annotation. Returns 0 if the point is inside.
pub fn annotation_distance(ann: &Annotation, point: Vec2) -> f32 {
    match ann {
        Annotation::Ship { pos, .. } => (*pos - point).length(),
        Annotation::FreehandStroke { points, .. } => {
            points.windows(2).map(|seg| point_to_segment_dist(point, seg[0], seg[1])).fold(f32::MAX, f32::min)
        }
        Annotation::Line { start, end, .. } => point_to_segment_dist(point, *start, *end),
        Annotation::Circle { center, radius, .. } => {
            let dist_from_center = (point - *center).length();
            if dist_from_center <= *radius { 0.0 } else { dist_from_center - *radius }
        }
        Annotation::Rectangle { center, half_size, rotation, .. } => {
            let dp = point - *center;
            let cos_r = rotation.cos();
            let sin_r = rotation.sin();
            let local = Vec2::new(dp.x * cos_r + dp.y * sin_r, -dp.x * sin_r + dp.y * cos_r);
            let dx = (local.x.abs() - half_size.x).max(0.0);
            let dy = (local.y.abs() - half_size.y).max(0.0);
            (dx * dx + dy * dy).sqrt()
        }
        Annotation::Triangle { center, radius, rotation, .. } => {
            let dist = (point - *center).length();
            let inradius = *radius * 0.5;
            if dist <= inradius {
                0.0
            } else {
                let verts: Vec<Vec2> = (0..3)
                    .map(|i| {
                        let angle = *rotation + i as f32 * std::f32::consts::TAU / 3.0 - std::f32::consts::FRAC_PI_2;
                        *center + Vec2::new(radius * angle.cos(), radius * angle.sin())
                    })
                    .collect();
                let mut min_dist = f32::MAX;
                for i in 0..3 {
                    let d = point_to_segment_dist(point, verts[i], verts[(i + 1) % 3]);
                    if d < min_dist {
                        min_dist = d;
                    }
                }
                min_dist
            }
        }
        Annotation::Arrow { points, .. } => {
            points.windows(2).map(|seg| point_to_segment_dist(point, seg[0], seg[1])).fold(f32::MAX, f32::min)
        }
        Annotation::Measurement { start, end, .. } => point_to_segment_dist(point, *start, *end),
    }
}

/// Distance from a point to a line segment.
pub fn point_to_segment_dist(p: Vec2, a: Vec2, b: Vec2) -> f32 {
    let ab = b - a;
    let ap = p - a;
    let len_sq = ab.length_sq();
    if len_sq < 0.001 {
        return ap.length();
    }
    let t = (ap.x * ab.x + ap.y * ab.y) / len_sq;
    let t = t.clamp(0.0, 1.0);
    let closest = a + ab * t;
    (p - closest).length()
}

// ─── Rendering ───────────────────────────────────────────────────────────────

/// Build a rotated textured quad mesh for a ship/plane icon.
pub fn make_rotated_icon_mesh(
    texture_id: egui::TextureId,
    center: Pos2,
    icon_size: f32,
    yaw: f32,
    tint: Color32,
) -> Shape {
    let half = icon_size / 2.0;
    let cos_r = yaw.sin();
    let sin_r = yaw.cos();

    let corners = [(-half, -half), (half, -half), (half, half), (-half, half)];
    let uvs = [(0.0, 0.0), (1.0, 0.0), (1.0, 1.0), (0.0, 1.0)];

    let mut mesh = egui::Mesh::with_texture(texture_id);
    for &(cx, cy) in &corners {
        let rx = cx * cos_r - cy * sin_r;
        let ry = cx * sin_r + cy * cos_r;
        mesh.vertices.push(egui::epaint::Vertex {
            pos: Pos2::new(center.x + rx, center.y + ry),
            uv: egui::Pos2::ZERO,
            color: tint,
        });
    }
    for (i, &(u, v)) in uvs.iter().enumerate() {
        mesh.vertices[i].uv = egui::pos2(u, v);
    }
    mesh.indices = vec![0, 1, 2, 0, 2, 3];
    Shape::Mesh(mesh.into())
}

/// Render a single annotation onto the map painter.
///
/// For `Measurement` annotations, `map_space_size` controls whether distances
/// are shown in kilometres (when `Some`) or raw pixels (when `None`).
pub fn render_annotation(
    ann: &Annotation,
    transform: &MapTransform,
    ship_icons: Option<&HashMap<String, TextureHandle>>,
    painter: &egui::Painter,
    map_space_size: Option<f32>,
) {
    match ann {
        Annotation::Ship { pos, yaw, species, friendly, config } => {
            let screen_pos = minimap_vec2_to_screen(*pos, transform);
            let icon_size = transform.scale_distance(ICON_SIZE_F32);
            let tint = if *friendly { FRIENDLY_COLOR } else { ENEMY_COLOR };
            if let Some(tex) = ship_icons.and_then(|icons| icons.get(species.as_str())) {
                painter.add(make_rotated_icon_mesh(tex.id(), screen_pos, icon_size, *yaw, tint));
            } else {
                painter.add(Shape::circle_filled(screen_pos, icon_size / 2.0, tint));
            }
            // Ship name label above the icon
            if let Some(cfg) = config {
                if !cfg.ship_name.is_empty() {
                    let label_pos = Pos2::new(screen_pos.x, screen_pos.y - icon_size / 2.0 - 4.0);
                    let font = game_font(transform.scale_distance(10.0));
                    let text = cfg.ship_name.as_str();
                    let galley = painter.layout_no_wrap(text.to_owned(), font.clone(), Color32::WHITE);
                    let text_offset = egui::vec2(galley.size().x / 2.0, galley.size().y);
                    // Shadow
                    painter.galley(
                        label_pos - text_offset + egui::vec2(1.0, 1.0),
                        galley.clone(),
                        Color32::from_black_alpha(180),
                    );
                    // Foreground
                    painter.galley(label_pos - text_offset, galley, Color32::WHITE);
                }
            }
        }
        Annotation::FreehandStroke { points, color, width } => {
            let stroke_w = transform.scale_stroke(*width);
            for pair in points.windows(2) {
                let a = minimap_vec2_to_screen(pair[0], transform);
                let b = minimap_vec2_to_screen(pair[1], transform);
                painter.add(Shape::LineSegment { points: [a, b], stroke: Stroke::new(stroke_w, *color) });
            }
        }
        Annotation::Line { start, end, color, width } => {
            let a = minimap_vec2_to_screen(*start, transform);
            let b = minimap_vec2_to_screen(*end, transform);
            painter.add(Shape::LineSegment {
                points: [a, b],
                stroke: Stroke::new(transform.scale_stroke(*width), *color),
            });
        }
        Annotation::Circle { center, radius, color, width, filled } => {
            let c = minimap_vec2_to_screen(*center, transform);
            let r = transform.scale_distance(*radius);
            if *filled {
                let [cr, cg, cb, _] = color.to_array();
                let fill = Color32::from_rgba_unmultiplied(cr, cg, cb, 38);
                painter.add(Shape::circle_filled(c, r, fill));
                painter.add(Shape::circle_stroke(c, r, Stroke::new(transform.scale_stroke(*width), *color)));
            } else {
                painter.add(Shape::circle_stroke(c, r, Stroke::new(transform.scale_stroke(*width), *color)));
            }
        }
        Annotation::Rectangle { center, half_size, rotation, color, width, filled } => {
            let cos_r = rotation.cos();
            let sin_r = rotation.sin();
            let corners_local = [
                Vec2::new(-half_size.x, -half_size.y),
                Vec2::new(half_size.x, -half_size.y),
                Vec2::new(half_size.x, half_size.y),
                Vec2::new(-half_size.x, half_size.y),
            ];
            let screen_corners: Vec<Pos2> = corners_local
                .iter()
                .map(|c| {
                    let rotated = Vec2::new(c.x * cos_r - c.y * sin_r, c.x * sin_r + c.y * cos_r);
                    minimap_vec2_to_screen(*center + rotated, transform)
                })
                .collect();
            if *filled {
                painter.add(Shape::convex_polygon(screen_corners, *color, Stroke::NONE));
            } else {
                let stroke = Stroke::new(transform.scale_stroke(*width), *color);
                painter.add(egui::epaint::PathShape::closed_line(screen_corners, stroke));
            }
        }
        Annotation::Triangle { center, radius, rotation, color, width, filled } => {
            let screen_verts: Vec<Pos2> = (0..3)
                .map(|i| {
                    let angle = *rotation + i as f32 * std::f32::consts::TAU / 3.0 - std::f32::consts::FRAC_PI_2;
                    let dx = radius * angle.cos();
                    let dy = radius * angle.sin();
                    minimap_vec2_to_screen(*center + Vec2::new(dx, dy), transform)
                })
                .collect();
            if *filled {
                painter.add(Shape::convex_polygon(screen_verts, *color, Stroke::NONE));
            } else {
                let stroke = Stroke::new(transform.scale_stroke(*width), *color);
                painter.add(egui::epaint::PathShape::closed_line(screen_verts, stroke));
            }
        }
        Annotation::Arrow { points, color, width } => {
            let stroke_w = transform.scale_stroke(*width);
            if points.len() >= 2 {
                let minimap_dir = arrow_direction_from_points(points);
                let tip_minimap = *points.last().unwrap();
                let tip = minimap_vec2_to_screen(tip_minimap, transform);
                let ref_pt = minimap_vec2_to_screen(tip_minimap - minimap_dir * 10.0, transform);
                let screen_dir = (tip - ref_pt).normalized();

                let arrow_len = transform.scale_distance(*width * 4.0).max(8.0);
                let perp = Vec2::new(-screen_dir.y, screen_dir.x);
                let wing = arrow_len * 0.5;

                let arrow_tip = tip + screen_dir * arrow_len;
                let left = tip + perp * wing;
                let right = tip - perp * wing;

                let screen_pts: Vec<Pos2> = points.iter().map(|p| minimap_vec2_to_screen(*p, transform)).collect();
                for pair in screen_pts.windows(2) {
                    painter
                        .add(Shape::LineSegment { points: [pair[0], pair[1]], stroke: Stroke::new(stroke_w, *color) });
                }

                painter.add(Shape::convex_polygon(vec![arrow_tip, left, right], *color, Stroke::NONE));
            } else if points.len() == 1 {
                let p = minimap_vec2_to_screen(points[0], transform);
                painter.add(Shape::circle_filled(p, stroke_w / 2.0, *color));
            }
        }
        Annotation::Measurement { start, end, color, width } => {
            let a = minimap_vec2_to_screen(*start, transform);
            let b = minimap_vec2_to_screen(*end, transform);
            painter.add(Shape::LineSegment {
                points: [a, b],
                stroke: Stroke::new(transform.scale_stroke(*width), *color),
            });
            render_measurement_details(*start, *end, *color, *width, transform, map_space_size, painter);
        }
    }
}

// ─── Arrow Direction ─────────────────────────────────────────────────────────

/// Compute a stable arrow direction from a sequence of minimap-space points.
pub fn arrow_direction_from_points(points: &[Vec2]) -> Vec2 {
    const ARROW_TRAILING_DISTANCE: f32 = 30.0;
    const MAX_TRAILING_POINTS: usize = 10;

    let n = points.len();
    if n < 2 {
        return Vec2::new(1.0, 0.0);
    }

    let tip = points[n - 1];
    let mut accumulated = Vec2::ZERO;
    let mut total_weight = 0.0;
    let mut distance_walked = 0.0;
    let trailing = n.min(MAX_TRAILING_POINTS + 1);

    for i in 1..trailing {
        let idx = n - 1 - i;
        let seg = points[idx + 1] - points[idx];
        let seg_len = seg.length();
        if seg_len < 0.001 {
            continue;
        }
        let seg_dir = seg / seg_len;
        let weight = 1.0 / (i as f32);
        accumulated += seg_dir * weight;
        total_weight += weight;
        distance_walked += seg_len;
        if distance_walked >= ARROW_TRAILING_DISTANCE {
            break;
        }
    }

    if total_weight > 0.0 {
        let avg = accumulated / total_weight;
        if avg.length() > 0.001 {
            return avg.normalized();
        }
    }

    let fallback = tip - points[0];
    if fallback.length() > 0.001 { fallback.normalized() } else { Vec2::new(1.0, 0.0) }
}

// ─── Freehand Smoothing ─────────────────────────────────────────────────────

/// Smooth a freehand polyline: RDP simplification + Chaikin subdivision.
pub fn smooth_freehand(points: Vec<Vec2>) -> Vec<Vec2> {
    if points.len() <= 2 {
        return points;
    }

    let bbox_diag = {
        let (mut lo, mut hi) = (points[0], points[0]);
        for p in &points {
            lo.x = lo.x.min(p.x);
            lo.y = lo.y.min(p.y);
            hi.x = hi.x.max(p.x);
            hi.y = hi.y.max(p.y);
        }
        (hi - lo).length()
    };
    let epsilon = (bbox_diag * 0.012).max(0.3);
    let simplified = rdp_simplify(&points, epsilon);

    let mut result = simplified;
    for _ in 0..2 {
        result = chaikin_subdivide(&result);
    }
    result
}

/// Ramer-Douglas-Peucker polyline simplification.
fn rdp_simplify(points: &[Vec2], epsilon: f32) -> Vec<Vec2> {
    if points.len() <= 2 {
        return points.to_vec();
    }
    let first = points[0];
    let last = *points.last().unwrap();
    let seg = last - first;
    let seg_len_sq = seg.length_sq();

    let mut max_dist = 0.0f32;
    let mut max_idx = 0;
    for (i, p) in points.iter().enumerate().skip(1).take(points.len() - 2) {
        let d = if seg_len_sq < 1e-10 {
            (*p - first).length()
        } else {
            let t = ((*p - first).dot(seg) / seg_len_sq).clamp(0.0, 1.0);
            (*p - (first + seg * t)).length()
        };
        if d > max_dist {
            max_dist = d;
            max_idx = i;
        }
    }

    if max_dist > epsilon {
        let mut left = rdp_simplify(&points[..=max_idx], epsilon);
        let right = rdp_simplify(&points[max_idx..], epsilon);
        left.pop();
        left.extend(right);
        left
    } else {
        vec![first, last]
    }
}

/// One pass of Chaikin's corner-cutting subdivision.
fn chaikin_subdivide(points: &[Vec2]) -> Vec<Vec2> {
    if points.len() <= 2 {
        return points.to_vec();
    }
    let mut out = Vec::with_capacity(points.len() * 2);
    out.push(points[0]);
    for pair in points.windows(2) {
        let (a, b) = (pair[0], pair[1]);
        out.push(a + (b - a) * 0.25);
        out.push(a + (b - a) * 0.75);
    }
    out.push(*points.last().unwrap());
    out
}

// ─── Measurement ─────────────────────────────────────────────────────────────

/// Convert a minimap-space distance to kilometres, given the map's space_size.
#[inline]
pub fn minimap_distance_to_km(minimap_dist: f32, space_size: f32) -> f32 {
    let bw = minimap_dist / MINIMAP_SIZE as f32 * space_size;
    bw * BW_TO_METERS / 1000.0
}

/// Convert kilometres to minimap-space distance, given the map's space_size.
#[inline]
pub fn km_to_minimap_distance(km: f32, space_size: f32) -> f32 {
    let bw = km * 1000.0 / BW_TO_METERS;
    bw / space_size * MINIMAP_SIZE as f32
}

/// Render measurement tick marks and distance labels along a line.
pub fn render_measurement_details(
    start: Vec2,
    end: Vec2,
    color: Color32,
    width: f32,
    transform: &MapTransform,
    map_space_size: Option<f32>,
    painter: &egui::Painter,
) {
    let screen_start = minimap_vec2_to_screen(start, transform);
    let screen_end = minimap_vec2_to_screen(end, transform);
    let minimap_dist = (end - start).length();
    if minimap_dist < 0.1 {
        return;
    }
    let dir = (end - start) / minimap_dist;
    let stroke_w = transform.scale_stroke(width);

    let font_size = transform.scale_distance(3.5).max(9.0);
    let label_font = FontId::proportional(font_size);

    let luma = 0.299 * color.r() as f32 + 0.587 * color.g() as f32 + 0.114 * color.b() as f32;
    let outline_color = if luma > 128.0 {
        Color32::from_rgba_unmultiplied(0, 0, 0, 200)
    } else {
        Color32::from_rgba_unmultiplied(255, 255, 255, 200)
    };

    let tick_color = Color32::from_rgba_unmultiplied(color.r(), color.g(), color.b(), 200);
    let small_tick_screen = transform.scale_distance(3.0);
    let large_tick_screen = transform.scale_distance(6.0);
    let tick_stroke = Stroke::new((stroke_w * 0.6).max(1.0), tick_color);
    let text_gap = (font_size * 0.4).max(4.0);
    let outline_d = (font_size * 0.1).clamp(1.0, 3.0);

    painter.add(Shape::circle_filled(screen_start, (stroke_w * 1.2).max(3.0), color));
    painter.add(Shape::circle_filled(screen_end, (stroke_w * 1.2).max(3.0), color));

    if let Some(space_size) = map_space_size {
        let total_km = minimap_distance_to_km(minimap_dist, space_size);
        let half_km_minimap = km_to_minimap_distance(0.5, space_size);

        let mut d = half_km_minimap;
        let mut tick_idx = 1u32;
        while d < minimap_dist {
            let tick_pos_minimap = start + dir * d;
            let tick_pos_screen = minimap_vec2_to_screen(tick_pos_minimap, transform);

            let is_full_km = tick_idx.is_multiple_of(2);
            let tick_half = if is_full_km { large_tick_screen } else { small_tick_screen };
            let screen_perp = Vec2::new(screen_end.x - screen_start.x, screen_end.y - screen_start.y);
            let screen_dir = screen_perp.normalized();
            let screen_perp = Vec2::new(-screen_dir.y, screen_dir.x);

            let p1 = tick_pos_screen + screen_perp * tick_half;
            let p2 = tick_pos_screen - screen_perp * tick_half;
            painter.add(Shape::LineSegment { points: [p1, p2], stroke: tick_stroke });

            if tick_idx.is_multiple_of(4) {
                let km_here = (tick_idx as f32) * 0.5;
                let label = format!("{:.0}", km_here);
                let label_offset = screen_perp * (large_tick_screen + font_size * 0.5 + text_gap);
                let label_pos = tick_pos_screen + label_offset;
                draw_outlined_text(
                    painter,
                    label_pos,
                    egui::Align2::CENTER_CENTER,
                    &label,
                    &label_font,
                    tick_color,
                    outline_color,
                    outline_d,
                );
            }

            d += half_km_minimap;
            tick_idx += 1;
        }

        let screen_line_dir = (screen_end - screen_start).normalized();
        let screen_line_perp = Vec2::new(-screen_line_dir.y, screen_line_dir.x);
        let label_offset = screen_line_perp * (large_tick_screen + font_size * 0.5 + text_gap);
        let total_label = format!("{:.1} km", total_km);
        let total_pos = screen_end + label_offset + screen_line_dir * 6.0;
        draw_outlined_text(
            painter,
            total_pos,
            egui::Align2::LEFT_CENTER,
            &total_label,
            &label_font,
            color,
            outline_color,
            outline_d,
        );
    } else {
        let screen_line_dir = (screen_end - screen_start).normalized();
        let screen_line_perp = Vec2::new(-screen_line_dir.y, screen_line_dir.x);
        let label_offset = screen_line_perp * (font_size * 0.5 + text_gap + 6.0);
        let label = format!("{:.0} px", minimap_dist);
        let label_pos = screen_end + label_offset;
        draw_outlined_text(
            painter,
            label_pos,
            egui::Align2::LEFT_CENTER,
            &label,
            &label_font,
            color,
            outline_color,
            outline_d,
        );
    }
}

/// Draw text with an 8-direction outline for contrast.
#[allow(clippy::too_many_arguments)]
fn draw_outlined_text(
    painter: &egui::Painter,
    pos: Pos2,
    anchor: egui::Align2,
    text: &str,
    font: &FontId,
    fg: Color32,
    outline: Color32,
    d: f32,
) {
    for off in [
        Vec2::new(-d, 0.0),
        Vec2::new(d, 0.0),
        Vec2::new(0.0, -d),
        Vec2::new(0.0, d),
        Vec2::new(-d, -d),
        Vec2::new(d, -d),
        Vec2::new(-d, d),
        Vec2::new(d, d),
    ] {
        painter.text(pos + off, anchor, text, font.clone(), outline);
    }
    painter.text(pos, anchor, text, font.clone(), fg);
}

// ─── Tool Preview ───────────────────────────────────────────────────────────

/// Render a preview of the active tool at the cursor position.
#[allow(clippy::too_many_arguments)]
pub fn render_tool_preview(
    tool: &PaintTool,
    minimap_pos: Vec2,
    color: Color32,
    stroke_width: f32,
    transform: &MapTransform,
    ship_icons: Option<&HashMap<String, TextureHandle>>,
    painter: &egui::Painter,
    map_space_size: Option<f32>,
) {
    let ghost_alpha = 128u8;
    match tool {
        PaintTool::PlacingShip { species, friendly, yaw } => {
            let screen_pos = minimap_vec2_to_screen(minimap_pos, transform);
            let icon_size = transform.scale_distance(ICON_SIZE_F32);
            let base = if *friendly { FRIENDLY_COLOR } else { ENEMY_COLOR };
            let tint = Color32::from_rgba_unmultiplied(base.r(), base.g(), base.b(), ghost_alpha);
            if let Some(tex) = ship_icons.and_then(|icons| icons.get(species.as_str())) {
                painter.add(make_rotated_icon_mesh(tex.id(), screen_pos, icon_size, *yaw, tint));
            } else {
                painter.add(Shape::circle_filled(screen_pos, icon_size / 2.0, tint));
            }
        }
        PaintTool::Freehand { current_stroke } => {
            if let Some(points) = current_stroke {
                let sw = transform.scale_stroke(stroke_width);
                for pair in points.windows(2) {
                    let a = minimap_vec2_to_screen(pair[0], transform);
                    let b = minimap_vec2_to_screen(pair[1], transform);
                    painter.add(Shape::LineSegment { points: [a, b], stroke: Stroke::new(sw, color) });
                }
            }
            let c = minimap_vec2_to_screen(minimap_pos, transform);
            let r = (transform.scale_stroke(stroke_width) / 2.0).max(3.0);
            painter.add(Shape::circle_stroke(c, r, Stroke::new(1.0, color)));
        }
        PaintTool::Eraser => {
            let c = minimap_vec2_to_screen(minimap_pos, transform);
            let r = transform.scale_distance(15.0);
            painter.add(Shape::circle_stroke(c, r, Stroke::new(1.5, Color32::from_rgb(255, 100, 100))));
        }
        PaintTool::DrawingLine { start, .. } => {
            let c = minimap_vec2_to_screen(minimap_pos, transform);
            let r = (transform.scale_stroke(stroke_width) / 2.0).max(3.0);
            painter.add(Shape::circle_stroke(c, r, Stroke::new(1.0, color)));
            if let Some(s) = start {
                let a = minimap_vec2_to_screen(*s, transform);
                let b = minimap_vec2_to_screen(minimap_pos, transform);
                let ghost_color = Color32::from_rgba_unmultiplied(color.r(), color.g(), color.b(), ghost_alpha);
                painter.add(Shape::LineSegment {
                    points: [a, b],
                    stroke: Stroke::new(transform.scale_stroke(stroke_width), ghost_color),
                });
            }
        }
        PaintTool::DrawingCircle { center: origin, filled, .. } => {
            let cursor_screen = minimap_vec2_to_screen(minimap_pos, transform);
            let r = (transform.scale_stroke(stroke_width) / 2.0).max(3.0);
            painter.add(Shape::circle_stroke(cursor_screen, r, Stroke::new(1.0, color)));
            if let Some(org) = origin {
                let radius = (minimap_pos - *org).length();
                let c = minimap_vec2_to_screen(*org, transform);
                let r = transform.scale_distance(radius);
                let ghost_color = Color32::from_rgba_unmultiplied(color.r(), color.g(), color.b(), ghost_alpha);
                if *filled {
                    painter.add(Shape::circle_filled(c, r, ghost_color));
                } else {
                    painter.add(Shape::circle_stroke(
                        c,
                        r,
                        Stroke::new(transform.scale_stroke(stroke_width), ghost_color),
                    ));
                }
            }
        }
        PaintTool::DrawingRect { center: origin, filled, .. } => {
            let cursor_screen = minimap_vec2_to_screen(minimap_pos, transform);
            let r = (transform.scale_stroke(stroke_width) / 2.0).max(3.0);
            painter.add(Shape::circle_stroke(cursor_screen, r, Stroke::new(1.0, color)));
            if let Some(org) = origin {
                let min = Vec2::new(org.x.min(minimap_pos.x), org.y.min(minimap_pos.y));
                let max = Vec2::new(org.x.max(minimap_pos.x), org.y.max(minimap_pos.y));
                let corners: Vec<Pos2> = [
                    Vec2::new(min.x, min.y),
                    Vec2::new(max.x, min.y),
                    Vec2::new(max.x, max.y),
                    Vec2::new(min.x, max.y),
                ]
                .iter()
                .map(|p| minimap_vec2_to_screen(*p, transform))
                .collect();
                let ghost_color = Color32::from_rgba_unmultiplied(color.r(), color.g(), color.b(), ghost_alpha);
                if *filled {
                    painter.add(Shape::convex_polygon(corners, ghost_color, Stroke::NONE));
                } else {
                    let stroke = Stroke::new(transform.scale_stroke(stroke_width), ghost_color);
                    painter.add(egui::epaint::PathShape::closed_line(corners, stroke));
                }
            }
        }
        PaintTool::DrawingTriangle { center, filled, .. } => {
            let cursor_screen = minimap_vec2_to_screen(minimap_pos, transform);
            let r = (transform.scale_stroke(stroke_width) / 2.0).max(3.0);
            painter.add(Shape::circle_stroke(cursor_screen, r, Stroke::new(1.0, color)));
            if let Some(ctr) = center {
                let radius = (minimap_pos - *ctr).length();
                let verts: Vec<Pos2> = (0..3)
                    .map(|i| {
                        let angle = i as f32 * std::f32::consts::TAU / 3.0 - std::f32::consts::FRAC_PI_2;
                        let dx = radius * angle.cos();
                        let dy = radius * angle.sin();
                        minimap_vec2_to_screen(*ctr + Vec2::new(dx, dy), transform)
                    })
                    .collect();
                let ghost_color = Color32::from_rgba_unmultiplied(color.r(), color.g(), color.b(), ghost_alpha);
                if *filled {
                    painter.add(Shape::convex_polygon(verts, ghost_color, Stroke::NONE));
                } else {
                    let stroke = Stroke::new(transform.scale_stroke(stroke_width), ghost_color);
                    painter.add(egui::epaint::PathShape::closed_line(verts, stroke));
                }
            }
        }
        PaintTool::DrawingArrow { current_stroke } => {
            if let Some(points) = current_stroke {
                let sw = transform.scale_stroke(stroke_width);
                let ghost_color = Color32::from_rgba_unmultiplied(color.r(), color.g(), color.b(), ghost_alpha);

                let mut full_path = points.clone();
                full_path.push(minimap_pos);

                if full_path.len() >= 2 {
                    let minimap_dir = arrow_direction_from_points(&full_path);
                    let tip = minimap_vec2_to_screen(minimap_pos, transform);
                    let ref_pt = minimap_vec2_to_screen(minimap_pos - minimap_dir * 10.0, transform);
                    let screen_dir = (tip - ref_pt).normalized();

                    let arrow_len = transform.scale_distance(stroke_width * 4.0).max(8.0);
                    let perp = Vec2::new(-screen_dir.y, screen_dir.x);
                    let wing = arrow_len * 0.5;

                    let arrow_tip = tip + screen_dir * arrow_len;
                    let left = tip + perp * wing;
                    let right = tip - perp * wing;

                    let screen_pts: Vec<Pos2> =
                        full_path.iter().map(|p| minimap_vec2_to_screen(*p, transform)).collect();
                    for pair in screen_pts.windows(2) {
                        painter.add(Shape::LineSegment {
                            points: [pair[0], pair[1]],
                            stroke: Stroke::new(sw, ghost_color),
                        });
                    }

                    painter.add(Shape::convex_polygon(vec![arrow_tip, left, right], ghost_color, Stroke::NONE));
                }
            }
            let c = minimap_vec2_to_screen(minimap_pos, transform);
            let r = (transform.scale_stroke(stroke_width) / 2.0).max(3.0);
            painter.add(Shape::circle_stroke(c, r, Stroke::new(1.0, color)));
        }
        PaintTool::DrawingMeasurement { start, .. } => {
            let c = minimap_vec2_to_screen(minimap_pos, transform);
            let r = (transform.scale_stroke(stroke_width) / 2.0).max(3.0);
            painter.add(Shape::circle_stroke(c, r, Stroke::new(1.0, color)));
            if let Some(s) = start {
                let a = minimap_vec2_to_screen(*s, transform);
                let b = minimap_vec2_to_screen(minimap_pos, transform);
                let ghost_color = Color32::from_rgba_unmultiplied(color.r(), color.g(), color.b(), ghost_alpha);
                painter.add(Shape::LineSegment {
                    points: [a, b],
                    stroke: Stroke::new(transform.scale_stroke(stroke_width), ghost_color),
                });
                render_measurement_details(
                    *s,
                    minimap_pos,
                    ghost_color,
                    stroke_width,
                    transform,
                    map_space_size,
                    painter,
                );
            }
        }
        PaintTool::None => {}
    }
}

// ─── Selection Highlight ─────────────────────────────────────────────────────

/// Render a selection highlight around an annotation (corner brackets + center crosshair
/// + rotation handle + measurement endpoint handles).
pub fn render_selection_highlight(ann: &Annotation, transform: &MapTransform, painter: &egui::Painter) {
    let highlight_stroke = Stroke::new(1.5, Color32::from_rgb(255, 255, 100));
    let bounds = annotation_screen_bounds(ann, transform);
    let expanded = bounds.expand(8.0);

    // Corner brackets
    let bracket_len = 8.0f32.min(expanded.width() / 3.0).min(expanded.height() / 3.0);
    for corner in [expanded.left_top(), expanded.right_top(), expanded.right_bottom(), expanded.left_bottom()] {
        let cx = if corner.x < expanded.center().x { 1.0 } else { -1.0 };
        let cy = if corner.y < expanded.center().y { 1.0 } else { -1.0 };
        painter.add(Shape::LineSegment {
            points: [corner, corner + Vec2::new(cx * bracket_len, 0.0)],
            stroke: highlight_stroke,
        });
        painter.add(Shape::LineSegment {
            points: [corner, corner + Vec2::new(0.0, cy * bracket_len)],
            stroke: highlight_stroke,
        });
    }

    // Center crosshair for shapes with a geometric center
    let center_minimap = match ann {
        Annotation::Circle { center, .. } => Some(*center),
        Annotation::Rectangle { center, .. } => Some(*center),
        Annotation::Triangle { center, .. } => Some(*center),
        _ => None,
    };
    if let Some(center) = center_minimap {
        let c = minimap_vec2_to_screen(center, transform);
        let arm = 5.0;
        let thin_stroke = Stroke::new(1.0, Color32::from_rgb(255, 255, 100));
        painter.add(Shape::LineSegment { points: [c - Vec2::X * arm, c + Vec2::X * arm], stroke: thin_stroke });
        painter.add(Shape::LineSegment { points: [c - Vec2::Y * arm, c + Vec2::Y * arm], stroke: thin_stroke });
    }

    // Rotation handle for rotatable annotations
    let has_rotation =
        matches!(ann, Annotation::Ship { .. } | Annotation::Rectangle { .. } | Annotation::Triangle { .. });
    if has_rotation {
        let (handle_pos, anchor) = rotation_handle_pos(ann, transform);
        let thin_stroke = Stroke::new(1.0, Color32::from_rgb(255, 255, 100));
        painter.add(Shape::LineSegment { points: [anchor, handle_pos], stroke: thin_stroke });
        painter.add(Shape::circle_filled(handle_pos, ROTATION_HANDLE_RADIUS, Color32::from_rgb(255, 255, 100)));
    }

    // Endpoint handles for measurement annotations
    if let Annotation::Measurement { start, end, .. } = ann {
        let handle_color = Color32::from_rgb(255, 255, 100);
        let start_screen = minimap_vec2_to_screen(*start, transform);
        let end_screen = minimap_vec2_to_screen(*end, transform);
        painter.add(Shape::circle_stroke(start_screen, 6.0, Stroke::new(1.5, handle_color)));
        painter.add(Shape::circle_stroke(end_screen, 6.0, Stroke::new(1.5, handle_color)));
    }
}

/// Get the screen position of the rotation handle and its anchor point.
pub fn rotation_handle_pos(ann: &Annotation, transform: &MapTransform) -> (Pos2, Pos2) {
    let bounds = annotation_screen_bounds(ann, transform);
    let anchor = Pos2::new(bounds.center().x, bounds.top());
    let handle = Pos2::new(anchor.x, anchor.y - ROTATION_HANDLE_DISTANCE);
    (handle, anchor)
}

/// Compute the screen-space bounding rect for an annotation.
pub fn annotation_screen_bounds(ann: &Annotation, transform: &MapTransform) -> Rect {
    match ann {
        Annotation::Ship { pos, .. } => {
            let c = minimap_vec2_to_screen(*pos, transform);
            let half = transform.scale_distance(ICON_SIZE_F32) / 2.0;
            Rect::from_center_size(c, egui::vec2(half * 2.0, half * 2.0))
        }
        Annotation::FreehandStroke { points, .. } => {
            let screen_pts: Vec<Pos2> = points.iter().map(|p| minimap_vec2_to_screen(*p, transform)).collect();
            let mut rect = Rect::from_min_max(screen_pts[0], screen_pts[0]);
            for p in &screen_pts[1..] {
                rect = rect.union(Rect::from_min_max(*p, *p));
            }
            rect
        }
        Annotation::Line { start, end, .. } => {
            let a = minimap_vec2_to_screen(*start, transform);
            let b = minimap_vec2_to_screen(*end, transform);
            Rect::from_two_pos(a, b)
        }
        Annotation::Circle { center, radius, .. } => {
            let c = minimap_vec2_to_screen(*center, transform);
            let r = transform.scale_distance(*radius);
            Rect::from_center_size(c, egui::vec2(r * 2.0, r * 2.0))
        }
        Annotation::Rectangle { center, half_size, rotation, .. } => {
            let cos_r = rotation.cos();
            let sin_r = rotation.sin();
            let corners_local = [
                Vec2::new(-half_size.x, -half_size.y),
                Vec2::new(half_size.x, -half_size.y),
                Vec2::new(half_size.x, half_size.y),
                Vec2::new(-half_size.x, half_size.y),
            ];
            let screen_corners: Vec<Pos2> = corners_local
                .iter()
                .map(|c| {
                    let rotated = Vec2::new(c.x * cos_r - c.y * sin_r, c.x * sin_r + c.y * cos_r);
                    minimap_vec2_to_screen(*center + rotated, transform)
                })
                .collect();
            let mut rect = Rect::from_min_max(screen_corners[0], screen_corners[0]);
            for p in &screen_corners[1..] {
                rect = rect.union(Rect::from_min_max(*p, *p));
            }
            rect
        }
        Annotation::Triangle { center, radius, rotation, .. } => {
            let screen_verts: Vec<Pos2> = (0..3)
                .map(|i| {
                    let angle = *rotation + i as f32 * std::f32::consts::TAU / 3.0 - std::f32::consts::FRAC_PI_2;
                    let dx = radius * angle.cos();
                    let dy = radius * angle.sin();
                    minimap_vec2_to_screen(*center + Vec2::new(dx, dy), transform)
                })
                .collect();
            let mut rect = Rect::from_min_max(screen_verts[0], screen_verts[0]);
            for p in &screen_verts[1..] {
                rect = rect.union(Rect::from_min_max(*p, *p));
            }
            rect
        }
        Annotation::Arrow { points, .. } => {
            let screen_pts: Vec<Pos2> = points.iter().map(|p| minimap_vec2_to_screen(*p, transform)).collect();
            let mut rect = Rect::from_min_max(screen_pts[0], screen_pts[0]);
            for p in &screen_pts[1..] {
                rect = rect.union(Rect::from_min_max(*p, *p));
            }
            rect
        }
        Annotation::Measurement { start, end, .. } => {
            let a = minimap_vec2_to_screen(*start, transform);
            let b = minimap_vec2_to_screen(*end, transform);
            Rect::from_two_pos(a, b)
        }
    }
}

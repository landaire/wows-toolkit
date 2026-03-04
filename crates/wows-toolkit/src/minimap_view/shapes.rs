//! Shared annotation rendering, hit testing, and geometry helpers.
//!
//! These functions are used by both the replay renderer and the tactics board.
//! Ship icon textures are optional — when absent, ships are rendered as simple
//! colored circles.

use std::collections::HashMap;
use std::sync::Arc;
use std::sync::mpsc;

use egui::Color32;
use egui::FontId;
use egui::Pos2;
use egui::Rect;
use egui::Shape;
use egui::Stroke;
use egui::TextureHandle;
use egui::Vec2;
use parking_lot::Mutex;

use wows_minimap_renderer::MINIMAP_SIZE;
use wows_minimap_renderer::MinimapPos;
use wows_minimap_renderer::assets::ICON_SIZE;

use crate::collab::peer::LocalEvent;
use wowsunpack::game_params::types::BigWorldDistance;
use wowsunpack::game_params::types::Km;

use super::Annotation;
use super::AnnotationState;
use super::ENEMY_COLOR;
use super::FRIENDLY_COLOR;
use super::MapTransform;
use super::PaintTool;
use super::SHIP_SPECIES;
use super::send_annotation_remove;
use super::send_annotation_update;

/// Default icon size as f32 (from minimap renderer).
pub const ICON_SIZE_F32: f32 = ICON_SIZE as f32;

pub const ROTATION_HANDLE_RADIUS: f32 = 5.0;
pub const ROTATION_HANDLE_DISTANCE: f32 = 25.0;

/// Font ID using the game font family (Warhelios Bold + CJK fallbacks).
/// Requires [`register_game_fonts`] to have been called at least once.
pub fn game_font(size: f32) -> FontId {
    FontId::new(size, egui::FontFamily::Name("GameFont".into()))
}

/// Add the `GameFont` family to `font_defs`.
///
/// If `game_fonts` is `Some`, the real Warhelios Bold + CJK fallback fonts are
/// inserted. Otherwise `GameFont` is aliased to the default proportional family
/// so that [`game_font`] never panics.
///
/// The caller is responsible for passing the result to `ctx.set_fonts()`.
pub fn register_game_fonts(
    font_defs: &mut egui::FontDefinitions,
    game_fonts: Option<&wows_minimap_renderer::GameFonts>,
) {
    if let Some(fonts) = game_fonts {
        font_defs
            .font_data
            .insert("game_font_primary".to_owned(), egui::FontData::from_owned(fonts.primary_bytes.clone()).into());
        let mut family_fonts = vec!["game_font_primary".to_owned()];
        let fallback_names = ["game_font_ko", "game_font_jp", "game_font_cn"];
        for (i, bytes) in fonts.fallback_bytes.iter().enumerate() {
            let name = fallback_names.get(i).unwrap_or(&"game_font_fallback").to_string();
            font_defs.font_data.insert(name.clone(), egui::FontData::from_owned(bytes.clone()).into());
            family_fonts.push(name);
        }
        font_defs.families.insert(egui::FontFamily::Name("GameFont".into()), family_fonts);
    } else if !font_defs.families.contains_key(&egui::FontFamily::Name("GameFont".into())) {
        let proportional = font_defs.families.get(&egui::FontFamily::Proportional).cloned().unwrap_or_default();
        font_defs.families.insert(egui::FontFamily::Name("GameFont".into()), proportional);
    }
}

/// Helper to convert a minimap `Vec2` position to screen `Pos2` via [`MapTransform`].
pub fn minimap_vec2_to_screen(pos: Vec2, transform: &MapTransform) -> Pos2 {
    transform.minimap_to_screen(&MinimapPos { x: pos.x as i32, y: pos.y as i32 })
}

// ─── Grid Overlay ───────────────────────────────────────────────────────────

/// Style parameters for the 10×10 minimap grid overlay.
pub struct GridStyle {
    pub grid_color: Color32,
    pub label_color: Color32,
    pub line_width: f32,
    pub label_font: FontId,
}

impl Default for GridStyle {
    fn default() -> Self {
        Self {
            grid_color: Color32::from_rgba_unmultiplied(180, 180, 180, 64),
            label_color: Color32::from_rgba_unmultiplied(200, 200, 200, 180),
            line_width: 0.5,
            label_font: FontId::proportional(9.0),
        }
    }
}

/// Draw a 10×10 grid overlay with coordinate labels (1-10 columns, A-J rows).
///
/// Line width and label font size are scaled through [`MapTransform::scale_stroke`]
/// so they stay legible at any zoom level.
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

    // Column labels (1-10 across the top)
    for i in 0..10 {
        let x = (i as f32 + 0.5) * cell;
        let pos = transform.minimap_to_screen(&MinimapPos { x: x as i32, y: (cell * 0.15) as i32 });
        painter.text(pos, egui::Align2::CENTER_CENTER, format!("{}", i + 1), font.clone(), style.label_color);
    }

    // Row labels (A-J down the left)
    for i in 0..10 {
        let y = (i as f32 + 0.5) * cell;
        let pos = transform.minimap_to_screen(&MinimapPos { x: (cell * 0.15) as i32, y: y as i32 });
        let label = (b'A' + i as u8) as char;
        painter.text(pos, egui::Align2::CENTER_CENTER, label.to_string(), font.clone(), style.label_color);
    }
}

// ─── Map Pings (click ripple) ───────────────────────────────────────────────

/// Duration in seconds that a ping ripple is visible.
pub const PING_DURATION: f32 = 1.0;

/// A click ripple on the map, in minimap coordinates.
pub struct MapPing {
    /// Position in minimap pixel coordinates.
    pub pos: [f32; 2],
    /// RGB colour of the ripple rings.
    pub color: [u8; 3],
    /// When the ping was created.
    pub time: std::time::Instant,
}

/// Draw active pings onto `painter` and return `true` if any were drawn
/// (so the caller can request a repaint). Expired pings (older than
/// [`PING_DURATION`]) are skipped; callers are responsible for pruning.
pub fn draw_pings(pings: &[MapPing], painter: &egui::Painter, transform: &MapTransform) -> bool {
    let now = std::time::Instant::now();
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

/// Draw remote peer cursors onto `painter`. Each cursor is rendered as a small
/// triangle arrow pointing up-left, with a name label and semi-transparent
/// background. Cursors fade out after 3 seconds and disappear after 5.
///
/// `my_user_id` is used to skip the local user's own cursor.
pub fn draw_remote_cursors(
    cursors: &[crate::collab::UserCursor],
    my_user_id: u64,
    painter: &egui::Painter,
    transform: &MapTransform,
) {
    let now = std::time::Instant::now();
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

        // Cursor arrow (small triangle pointing up-left)
        let size = 10.0;
        let points =
            vec![screen_pos, screen_pos + Vec2::new(0.0, size * 1.5), screen_pos + Vec2::new(size * 0.6, size * 1.1)];
        painter.add(Shape::convex_polygon(
            points,
            color,
            Stroke::new(1.0, Color32::from_rgba_unmultiplied(0, 0, 0, alpha)),
        ));

        // Name label with background
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

/// Short display name for ship species (used in context menu buttons).
pub fn ship_short_name(species: &str) -> &str {
    match species {
        "Destroyer" => "DD",
        "Cruiser" => "CA",
        "Battleship" => "BB",
        "AirCarrier" => "CV",
        "Submarine" => "SS",
        _ => species,
    }
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
            uv: egui::Pos2::ZERO, // filled below
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
/// `ship_icons` is optional — when `None`, ship annotations fall back to
/// colored circles. Pass `Some(&textures.ship_icons)` in the replay renderer.
pub fn render_annotation(
    ann: &Annotation,
    transform: &MapTransform,
    ship_icons: Option<&HashMap<String, TextureHandle>>,
    painter: &egui::Painter,
) {
    match ann {
        Annotation::Ship { pos, yaw, species, friendly } => {
            let screen_pos = minimap_vec2_to_screen(*pos, transform);
            let icon_size = transform.scale_distance(ICON_SIZE_F32);
            let tint = if *friendly { FRIENDLY_COLOR } else { ENEMY_COLOR };
            if let Some(tex) = ship_icons.and_then(|icons| icons.get(species.as_str())) {
                painter.add(make_rotated_icon_mesh(tex.id(), screen_pos, icon_size, *yaw, tint));
            } else {
                // Fallback: filled circle when no icon texture is available.
                painter.add(Shape::circle_filled(screen_pos, icon_size / 2.0, tint));
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
                // Semi-transparent fill (15% alpha) + full-opacity outline.
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
    }
}

/// Render a preview of the active tool at the cursor position.
pub fn render_tool_preview(
    tool: &PaintTool,
    minimap_pos: Vec2,
    color: Color32,
    stroke_width: f32,
    transform: &MapTransform,
    ship_icons: Option<&HashMap<String, TextureHandle>>,
    painter: &egui::Painter,
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
        PaintTool::None => {}
    }
}

/// Render a selection highlight around an annotation (corner brackets + rotation handle).
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
}

/// Get the screen position of the rotation handle and its anchor point.
///
/// The handle is always at the top-center of the axis-aligned bounding box.
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
    }
}

// ─── Annotation Tool Buttons ────────────────────────────────────────────────

/// Draw ship species buttons (friendly + enemy rows) in the annotation toolbar.
///
/// When icon textures are available, buttons show 24×24 rotated ship icons
/// tinted with the team colour; otherwise falls back to text abbreviations.
///
/// Modifies `ann.active_tool` when a button is clicked.
pub fn draw_ship_species_buttons(
    ui: &mut egui::Ui,
    ann: &mut super::AnnotationState,
    ship_icons: Option<&HashMap<String, TextureHandle>>,
) {
    ui.label(egui::RichText::new("Friendly Ships").color(FRIENDLY_COLOR).small());
    ui.horizontal(|ui| {
        for species in &SHIP_SPECIES {
            if ship_species_button(ui, species, FRIENDLY_COLOR, ship_icons) {
                ann.active_tool = PaintTool::PlacingShip { species: species.to_string(), friendly: true, yaw: 0.0 };
                ann.show_context_menu = false;
            }
        }
    });

    ui.label(egui::RichText::new("Enemy Ships").color(ENEMY_COLOR).small());
    ui.horizontal(|ui| {
        for species in &SHIP_SPECIES {
            if ship_species_button(ui, species, ENEMY_COLOR, ship_icons) {
                ann.active_tool = PaintTool::PlacingShip { species: species.to_string(), friendly: false, yaw: 0.0 };
                ann.show_context_menu = false;
            }
        }
    });
}

/// Single ship species button: icon if available, text fallback otherwise.
fn ship_species_button(
    ui: &mut egui::Ui,
    species: &str,
    tint: Color32,
    ship_icons: Option<&HashMap<String, TextureHandle>>,
) -> bool {
    if let Some(tex) = ship_icons.and_then(|icons| icons.get(species)) {
        let img = egui::Image::new(egui::load::SizedTexture::new(tex.id(), egui::vec2(24.0, 24.0)))
            .rotate(std::f32::consts::FRAC_PI_2, egui::vec2(0.5, 0.5))
            .tint(tint);
        ui.add(egui::Button::image(img)).on_hover_text(ship_short_name(species)).clicked()
    } else {
        ui.button(egui::RichText::new(ship_short_name(species)).color(tint)).clicked()
    }
}

// ─── Shared Annotation Tool Interaction ────────────────────────────────────

/// Result of processing an annotation tool interaction for one frame.
pub struct ToolInteractionResult {
    /// A new annotation to add.
    pub new_annotation: Option<Annotation>,
    /// Index of an annotation to erase (Eraser tool).
    pub erase_index: Option<usize>,
}

/// Process the active paint tool (PlacingShip, Freehand, Eraser, Line, Circle,
/// Rect, Triangle) for one frame. Returns any new annotation to add or erase
/// index, but does NOT mutate the annotation list — callers handle that plus
/// collab sync.
pub fn handle_tool_interaction(
    ann: &mut AnnotationState,
    response: &egui::Response,
    transform: &MapTransform,
) -> ToolInteractionResult {
    let cursor_minimap = response.hover_pos().map(|p| transform.screen_to_minimap(p));
    let paint_color = ann.paint_color;
    let stroke_width = ann.stroke_width;
    let mut new_annotation: Option<Annotation> = None;
    let mut erase_index: Option<usize> = None;

    match &mut ann.active_tool {
        PaintTool::PlacingShip { species, friendly, yaw } => {
            if response.clicked()
                && let Some(pos) = cursor_minimap
            {
                new_annotation =
                    Some(Annotation::Ship { pos, yaw: *yaw, species: species.clone(), friendly: *friendly });
            }
        }
        PaintTool::Freehand { current_stroke } => {
            if response.dragged_by(egui::PointerButton::Primary)
                && let Some(pos) = cursor_minimap
            {
                current_stroke.get_or_insert_with(Vec::new).push(pos);
            }
            if response.drag_stopped_by(egui::PointerButton::Primary)
                && let Some(points) = current_stroke.take()
                && points.len() >= 2
            {
                new_annotation = Some(Annotation::FreehandStroke { points, color: paint_color, width: stroke_width });
            }
        }
        PaintTool::Eraser => {
            if response.clicked()
                && let Some(click_pos) = cursor_minimap
            {
                let threshold = 15.0;
                let mut closest_idx = None;
                let mut closest_dist = f32::MAX;
                for (i, a) in ann.annotations.iter().enumerate() {
                    let d = annotation_distance(a, click_pos);
                    if d < closest_dist {
                        closest_dist = d;
                        closest_idx = Some(i);
                    }
                }
                if closest_dist < threshold {
                    erase_index = closest_idx;
                }
            }
        }
        PaintTool::DrawingLine { start } => {
            if response.clicked()
                && let Some(pos) = cursor_minimap
            {
                if let Some(s) = *start {
                    new_annotation =
                        Some(Annotation::Line { start: s, end: pos, color: paint_color, width: stroke_width });
                    *start = None;
                } else {
                    *start = Some(pos);
                }
            }
        }
        PaintTool::DrawingCircle { filled, center } => {
            if response.drag_started_by(egui::PointerButton::Primary)
                && let Some(pos) = cursor_minimap
            {
                *center = Some(pos);
            }
            if response.drag_stopped_by(egui::PointerButton::Primary)
                && let Some(origin) = *center
            {
                if let Some(pos) = cursor_minimap {
                    let radius = (pos - origin).length();
                    if radius > 1.0 {
                        new_annotation = Some(Annotation::Circle {
                            center: origin,
                            radius,
                            color: paint_color,
                            width: stroke_width,
                            filled: *filled,
                        });
                    }
                }
                *center = None;
            }
        }
        PaintTool::DrawingRect { filled, center } => {
            if response.drag_started_by(egui::PointerButton::Primary)
                && let Some(pos) = cursor_minimap
            {
                *center = Some(pos);
            }
            if response.drag_stopped_by(egui::PointerButton::Primary)
                && let Some(origin) = *center
            {
                if let Some(pos) = cursor_minimap {
                    let mid = (origin + pos) / 2.0;
                    let half = ((pos - origin) / 2.0).abs();
                    if half.x > 1.0 && half.y > 1.0 {
                        new_annotation = Some(Annotation::Rectangle {
                            center: mid,
                            half_size: half,
                            rotation: 0.0,
                            color: paint_color,
                            width: stroke_width,
                            filled: *filled,
                        });
                    }
                }
                *center = None;
            }
        }
        PaintTool::DrawingTriangle { filled, center } => {
            if response.clicked()
                && let Some(pos) = cursor_minimap
            {
                if let Some(ctr) = *center {
                    let radius = (pos - ctr).length();
                    new_annotation = Some(Annotation::Triangle {
                        center: ctr,
                        radius,
                        rotation: 0.0,
                        color: paint_color,
                        width: stroke_width,
                        filled: *filled,
                    });
                    *center = None;
                } else {
                    *center = Some(pos);
                }
            }
        }
        PaintTool::None => {}
    }

    ToolInteractionResult { new_annotation, erase_index }
}

// ─── Shared Annotation Select / Move / Rotate ──────────────────────────────

/// Result of annotation selection/move/rotate for one frame.
pub struct SelectMoveResult {
    /// Index of annotation that was moved (for collab sync on drag-stop).
    pub moved_index: Option<usize>,
    /// Index of annotation whose rotation drag just stopped.
    pub rotation_stopped_index: Option<usize>,
    /// Index of annotation that was selected by click.
    pub selected_by_click: bool,
}

/// Handle annotation select, move, and rotate when no drawing tool is active.
///
/// Manages rotation handle drag, click-to-select, and drag-to-move.
/// Returns info about what changed so callers can sync to collab.
pub fn handle_annotation_select_move(
    ann: &mut AnnotationState,
    response: &egui::Response,
    transform: &MapTransform,
) -> SelectMoveResult {
    let mut result = SelectMoveResult { moved_index: None, rotation_stopped_index: None, selected_by_click: false };

    // Check if drag started on the rotation handle
    if response.drag_started_by(egui::PointerButton::Primary)
        && let Some(sel) = ann.selected_index
        && sel < ann.annotations.len()
    {
        let has_rot = matches!(
            ann.annotations[sel],
            Annotation::Ship { .. } | Annotation::Rectangle { .. } | Annotation::Triangle { .. }
        );
        if has_rot && let Some(drag_origin) = response.interact_pointer_pos() {
            let (handle, _) = rotation_handle_pos(&ann.annotations[sel], transform);
            if (drag_origin - handle).length() < ROTATION_HANDLE_RADIUS + 8.0 {
                ann.dragging_rotation = true;
            }
        }
    }

    // Handle rotation drag
    if ann.dragging_rotation
        && response.dragged_by(egui::PointerButton::Primary)
        && let Some(sel) = ann.selected_index
        && sel < ann.annotations.len()
        && let Some(cursor_screen) = response.hover_pos()
    {
        let center_screen = annotation_screen_bounds(&ann.annotations[sel], transform).center();
        let angle = -(cursor_screen.x - center_screen.x).atan2(-(cursor_screen.y - center_screen.y));
        match &mut ann.annotations[sel] {
            Annotation::Ship { yaw, .. } => *yaw = angle,
            Annotation::Rectangle { rotation, .. } => *rotation = angle,
            Annotation::Triangle { rotation, .. } => *rotation = angle,
            _ => {}
        }
    }

    // Stop rotation drag
    if ann.dragging_rotation && response.drag_stopped_by(egui::PointerButton::Primary) {
        ann.dragging_rotation = false;
        result.rotation_stopped_index = ann.selected_index;
    }

    // Click to select/deselect annotations
    if response.clicked()
        && let Some(click_pos) = response.hover_pos().map(|p| transform.screen_to_minimap(p))
    {
        let threshold = 15.0;
        let mut closest_idx = None;
        let mut closest_dist = f32::MAX;
        for (i, a) in ann.annotations.iter().enumerate() {
            let d = annotation_distance(a, click_pos);
            if d < closest_dist {
                closest_dist = d;
                closest_idx = Some(i);
            }
        }
        if closest_dist < threshold {
            ann.selected_index = closest_idx;
        } else {
            ann.selected_index = None;
        }
        result.selected_by_click = true;
    }

    // Drag to move selected annotation (only if not rotating)
    if !ann.dragging_rotation
        && response.dragged_by(egui::PointerButton::Primary)
        && let Some(sel) = ann.selected_index
        && sel < ann.annotations.len()
    {
        let delta = response.drag_delta();
        let minimap_delta = Vec2::new(
            delta.x / (transform.window_scale * transform.zoom),
            delta.y / (transform.window_scale * transform.zoom),
        );
        match &mut ann.annotations[sel] {
            Annotation::Ship { pos, .. } => *pos += minimap_delta,
            Annotation::FreehandStroke { points, .. } => {
                for p in points.iter_mut() {
                    *p += minimap_delta;
                }
            }
            Annotation::Line { start, end, .. } => {
                *start += minimap_delta;
                *end += minimap_delta;
            }
            Annotation::Circle { center, .. } => *center += minimap_delta,
            Annotation::Rectangle { center, .. } => *center += minimap_delta,
            Annotation::Triangle { center, .. } => *center += minimap_delta,
        }
        result.moved_index = Some(sel);
    }

    // Sync moved annotation on drag release
    if !ann.dragging_rotation
        && response.drag_stopped_by(egui::PointerButton::Primary)
        && let Some(sel) = ann.selected_index
        && sel < ann.annotations.len()
    {
        result.moved_index = Some(sel);
    }

    result
}

// ─── Shared Context Menu Drawing ───────────────────────────────────────────

/// Preset colors for annotation color picker.
pub const PRESET_COLORS: &[(Color32, &str)] = &[
    (Color32::WHITE, "White"),
    (Color32::from_rgb(160, 160, 160), "Gray"),
    (Color32::from_rgb(230, 50, 50), "Red"),
    (Color32::from_rgb(240, 140, 30), "Orange"),
    (Color32::from_rgb(240, 230, 50), "Yellow"),
    (Color32::from_rgb(50, 200, 50), "Green"),
    (Color32::from_rgb(50, 120, 230), "Blue"),
    (Color32::from_rgb(180, 60, 230), "Purple"),
    (Color32::from_rgb(255, 130, 180), "Pink"),
];

/// Result of drawing the common annotation context menu.
pub struct AnnotationMenuResult {
    /// `true` if the undo button was clicked (caller may need to sync collab).
    pub did_undo: bool,
    /// `true` if the "Clear All" button was clicked (caller may need to sync collab).
    pub did_clear: bool,
}

/// Draw the common annotation context menu items: ship placement buttons,
/// drawing tool buttons, color presets, stroke width slider, and undo/clear.
///
/// Returns which actions were taken so the caller can send collab events.
pub fn draw_annotation_menu_common(
    ui: &mut egui::Ui,
    ann: &mut AnnotationState,
    ship_icons: Option<&HashMap<String, TextureHandle>>,
) -> AnnotationMenuResult {
    use crate::icons;

    draw_ship_species_buttons(ui, ann, ship_icons);

    ui.separator();

    // ── Drawing tools row ──
    ui.label(egui::RichText::new("Drawing Tools").small());
    ui.horizontal(|ui| {
        if ui.button(icons::PAINT_BRUSH).on_hover_text("Freehand").clicked() {
            ann.active_tool = PaintTool::Freehand { current_stroke: None };
            ann.show_context_menu = false;
        }
        if ui.button(icons::ERASER).on_hover_text("Eraser").clicked() {
            ann.active_tool = PaintTool::Eraser;
            ann.show_context_menu = false;
        }
        if ui.button(icons::LINE_SEGMENT).on_hover_text("Line").clicked() {
            ann.active_tool = PaintTool::DrawingLine { start: None };
            ann.show_context_menu = false;
        }
        if ui.button(icons::CIRCLE).on_hover_text("Circle").clicked() {
            ann.active_tool = PaintTool::DrawingCircle { filled: false, center: None };
            ann.show_context_menu = false;
        }
        if ui.button(icons::SQUARE).on_hover_text("Rectangle").clicked() {
            ann.active_tool = PaintTool::DrawingRect { filled: false, center: None };
            ann.show_context_menu = false;
        }
        if ui.button(icons::TRIANGLE).on_hover_text("Triangle").clicked() {
            ann.active_tool = PaintTool::DrawingTriangle { filled: false, center: None };
            ann.show_context_menu = false;
        }
    });

    ui.separator();

    // ── Color presets + stroke width ──
    ui.horizontal(|ui| {
        let swatch_size = egui::vec2(16.0, 16.0);
        egui::color_picker::color_edit_button_srgba(ui, &mut ann.paint_color, egui::color_picker::Alpha::Opaque);
        ui.add_space(4.0);
        for &(color, name) in PRESET_COLORS {
            let selected = ann.paint_color == color;
            let (rect, resp) = ui.allocate_exact_size(swatch_size, egui::Sense::click());
            ui.painter().rect_filled(rect, egui::CornerRadius::same(3), color);
            if selected {
                ui.painter().rect_stroke(
                    rect,
                    egui::CornerRadius::same(3),
                    Stroke::new(2.0, Color32::WHITE),
                    egui::StrokeKind::Outside,
                );
            }
            if resp.on_hover_text(name).clicked() {
                ann.paint_color = color;
            }
        }
    });
    ui.horizontal(|ui| {
        ui.label("Width:");
        ui.add(egui::Slider::new(&mut ann.stroke_width, 1.0..=8.0).max_decimals(1));
    });

    ui.separator();

    // ── Undo / Clear ──
    let mut did_undo = false;
    let mut did_clear = false;
    ui.horizontal(|ui| {
        if ui.button("Undo").clicked() {
            ann.undo();
            ann.show_context_menu = false;
            did_undo = true;
        }
        if ui.button("Clear All").clicked() {
            ann.save_undo();
            ann.annotations.clear();
            ann.annotation_ids.clear();
            ann.annotation_owners.clear();
            ann.selected_index = None;
            ann.show_context_menu = false;
            did_clear = true;
        }
    });
    AnnotationMenuResult { did_undo, did_clear }
}

/// Format the active tool as a human-readable label.
pub fn tool_label(tool: &PaintTool) -> Option<String> {
    match tool {
        PaintTool::None => None,
        PaintTool::PlacingShip { species, friendly, .. } => {
            let team = if *friendly { "Friendly" } else { "Enemy" };
            Some(format!("Placing {} {}", team, ship_short_name(species)))
        }
        PaintTool::Freehand { .. } => Some("Freehand".into()),
        PaintTool::Eraser => Some("Eraser".into()),
        PaintTool::DrawingLine { .. } => Some("Line".into()),
        PaintTool::DrawingCircle { .. } => Some("Circle".into()),
        PaintTool::DrawingRect { .. } => Some("Rectangle".into()),
        PaintTool::DrawingTriangle { .. } => Some("Triangle".into()),
    }
}

/// Determine the cursor icon for the current annotation tool state.
///
/// Returns `None` when no specific cursor should be set (caller may apply
/// zoom-dependent cursors). `response` and `transform` are used to check
/// whether the cursor is hovering a rotation handle.
pub fn annotation_cursor_icon(
    ann: &AnnotationState,
    response: &egui::Response,
    transform: &MapTransform,
) -> Option<egui::CursorIcon> {
    match &ann.active_tool {
        PaintTool::PlacingShip { .. } => Some(egui::CursorIcon::Cell),
        PaintTool::Freehand { .. }
        | PaintTool::Eraser
        | PaintTool::DrawingLine { .. }
        | PaintTool::DrawingCircle { .. }
        | PaintTool::DrawingRect { .. }
        | PaintTool::DrawingTriangle { .. } => Some(egui::CursorIcon::Crosshair),
        PaintTool::None => {
            if let Some(sel) = ann.selected_index {
                if ann.dragging_rotation {
                    Some(egui::CursorIcon::Grabbing)
                } else if sel < ann.annotations.len() {
                    // Check if hovering the rotation handle
                    let has_rot = matches!(
                        ann.annotations[sel],
                        Annotation::Ship { .. } | Annotation::Rectangle { .. } | Annotation::Triangle { .. }
                    );
                    let on_handle = has_rot
                        && response.hover_pos().is_some_and(|hp| {
                            let (handle, _) = rotation_handle_pos(&ann.annotations[sel], transform);
                            (hp - handle).length() < ROTATION_HANDLE_RADIUS + 8.0
                        });
                    if on_handle { Some(egui::CursorIcon::Alias) } else { Some(egui::CursorIcon::Grab) }
                } else {
                    Some(egui::CursorIcon::Grab)
                }
            } else {
                None
            }
        }
    }
}

/// Handle scroll-wheel yaw rotation for the PlacingShip tool.
/// Returns `true` if the scroll was consumed (i.e. yaw was adjusted).
pub fn handle_scroll_yaw(ann: &mut AnnotationState, scroll_delta: f32) -> bool {
    if scroll_delta == 0.0 {
        return false;
    }
    match &mut ann.active_tool {
        PaintTool::PlacingShip { yaw, .. } => {
            *yaw += scroll_delta * 0.005;
            true
        }
        _ => false,
    }
}

/// Draw the annotation selection edit popup (size, color, filled, team, delete).
///
/// Shared between the replay renderer and the tactics board. The popup appears
/// to the right of the selected annotation's screen bounds.
///
/// Call this after checking `selected_index` and computing `bounds` via
/// [`annotation_screen_bounds`]. The function locks `annotation_arc` internally
/// (the caller must drop any prior lock before calling).
pub fn draw_annotation_edit_popup(
    ctx: &egui::Context,
    area_id: egui::Id,
    annotation_arc: &Arc<Mutex<AnnotationState>>,
    sel_idx: usize,
    bounds: Rect,
    map_space_size: Option<f32>,
    collab_local_tx: &Option<mpsc::Sender<LocalEvent>>,
) {
    use crate::icons;

    let popup_pos = Pos2::new(bounds.right() + 8.0, bounds.center().y);
    egui::Area::new(area_id).order(egui::Order::Foreground).fixed_pos(popup_pos).interactable(true).show(ctx, |ui| {
        let frame = egui::Frame::NONE
            .fill(Color32::from_gray(30))
            .corner_radius(egui::CornerRadius::same(6))
            .inner_margin(egui::Margin::same(6))
            .stroke(Stroke::new(1.0, Color32::from_gray(80)));
        frame.show(ui, |ui| {
            let mut ann = annotation_arc.lock();
            if sel_idx >= ann.annotations.len() {
                return;
            }

            // Size slider (for circle, rect, triangle)
            let has_size = matches!(
                ann.annotations[sel_idx],
                Annotation::Circle { .. } | Annotation::Rectangle { .. } | Annotation::Triangle { .. }
            );
            if has_size {
                ui.horizontal(|ui| {
                    ui.label(egui::RichText::new("Size").small());
                    let is_circle = matches!(&ann.annotations[sel_idx], Annotation::Circle { .. });
                    let use_km = is_circle && map_space_size.is_some();
                    let mut size = match &ann.annotations[sel_idx] {
                        Annotation::Circle { radius, .. } => *radius,
                        Annotation::Rectangle { half_size, .. } => (half_size.x + half_size.y) / 2.0,
                        Annotation::Triangle { radius, .. } => *radius,
                        _ => 0.0,
                    };
                    let old = size;
                    if use_km {
                        let space_size = map_space_size.unwrap();
                        let bw = size / 768.0 * space_size;
                        let mut km = BigWorldDistance::from(bw).to_km().value();
                        let old_km = km;
                        ui.add(
                            egui::DragValue::new(&mut km).speed(0.1).range(0.1..=20.0).fixed_decimals(1).suffix(" km"),
                        );
                        if km != old_km {
                            size = Km::from(km).to_bigworld().value() / space_size * 768.0;
                        }
                    } else {
                        ui.add(egui::DragValue::new(&mut size).speed(1.0).range(5.0..=500.0));
                    }
                    if size != old && size > 0.0 {
                        match &mut ann.annotations[sel_idx] {
                            Annotation::Circle { radius, .. } => *radius = size,
                            Annotation::Rectangle { half_size, .. } => {
                                let ratio = if old > 0.0 { size / old } else { 1.0 };
                                *half_size *= ratio;
                            }
                            Annotation::Triangle { radius, .. } => *radius = size,
                            _ => {}
                        }
                        send_annotation_update(collab_local_tx, &ann, sel_idx);
                    }
                });
            }

            // Color picker (for non-ship annotations)
            let is_ship = matches!(ann.annotations[sel_idx], Annotation::Ship { .. });
            if !is_ship {
                ui.horizontal(|ui| {
                    ui.label(egui::RichText::new("Color").small());
                    let color_ref = match &mut ann.annotations[sel_idx] {
                        Annotation::FreehandStroke { color, .. } => color,
                        Annotation::Line { color, .. } => color,
                        Annotation::Circle { color, .. } => color,
                        Annotation::Rectangle { color, .. } => color,
                        Annotation::Triangle { color, .. } => color,
                        _ => unreachable!(),
                    };
                    let old_color = *color_ref;
                    egui::color_picker::color_edit_button_srgba(ui, color_ref, egui::color_picker::Alpha::Opaque);
                    if *color_ref != old_color {
                        send_annotation_update(collab_local_tx, &ann, sel_idx);
                    }
                });
            }

            // Filled toggle (for circle, rect, triangle)
            let has_filled = matches!(
                ann.annotations[sel_idx],
                Annotation::Circle { .. } | Annotation::Rectangle { .. } | Annotation::Triangle { .. }
            );
            if has_filled {
                let filled_ref = match &mut ann.annotations[sel_idx] {
                    Annotation::Circle { filled, .. } => filled,
                    Annotation::Rectangle { filled, .. } => filled,
                    Annotation::Triangle { filled, .. } => filled,
                    _ => unreachable!(),
                };
                let old_filled = *filled_ref;
                ui.checkbox(filled_ref, egui::RichText::new("Filled").small());
                if *filled_ref != old_filled {
                    send_annotation_update(collab_local_tx, &ann, sel_idx);
                }
            }

            // Team toggle (for ships)
            if is_ship && let Annotation::Ship { friendly, .. } = &mut ann.annotations[sel_idx] {
                let (label, color) = if *friendly { ("Friendly", FRIENDLY_COLOR) } else { ("Enemy  ", ENEMY_COLOR) };
                let btn =
                    egui::Button::new(egui::RichText::new(label).color(color).small()).min_size(egui::vec2(60.0, 0.0));
                if ui.add(btn).clicked() {
                    *friendly = !*friendly;
                    send_annotation_update(collab_local_tx, &ann, sel_idx);
                }
            }

            // Delete button
            if ui
                .button(egui::RichText::new(icons::TRASH).color(Color32::from_rgb(255, 100, 100)))
                .on_hover_text("Delete")
                .clicked()
            {
                ann.save_undo();
                let id = ann.annotation_ids[sel_idx];
                ann.annotations.remove(sel_idx);
                ann.annotation_ids.remove(sel_idx);
                ann.annotation_owners.remove(sel_idx);
                ann.selected_index = None;
                send_annotation_remove(collab_local_tx, id);
            }
        });
    });
}

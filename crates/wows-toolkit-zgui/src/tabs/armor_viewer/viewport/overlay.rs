//! What is drawn over the scene in screen space, so it keeps its size however far the camera pulls
//! back: the splash box names, the length marks' distances, the hardpoint names, the cast arcs,
//! the rings from landing hits and the navigation gizmo.
//!
//! The model is projected into an [`OverlayScene`] in CSS pixels whenever it changes, and a custom
//! element paints that scene with the window's own quads, paths and glyph runs.

use std::collections::HashMap;
use std::sync::Arc;
use std::time::Instant;

use wows_toolkit_armor::length_marks::MarkLabels;
use wows_toolkit_armor::trajectory;
use wows_toolkit_armor::viewport::camera::Axis;
use wows_toolkit_armor::viewport::gizmo;
use wows_toolkit_armor::viewport::types::Vec2;
use wows_toolkit_armor::viewport::types::Vec3;
use wows_toolkit_armor::waterline_grid;
use zgui::app::LineRequest;
use zgui::app::Shaper;
use zgui::canvas::Ink;
use zgui::canvas::Paint;
use zgui::canvas::Shape;
use zgui::canvas::Stroke;
use zgui::canvas::zgui_color::Color;
use zgui::custom::CustomElement;
use zgui::custom::CustomLayoutCx;
use zgui::custom::CustomMeasured;
use zgui::custom::PaintSlot;
use zgui::custom::ScenePainter;
use zgui::custom::ShapedRunOwned;
use zgui::geom::Device;
use zgui::geom::DevicePx;
use zgui::geom::Point;
use zgui::geom::Rect;
use zgui::geom::Size;
use zgui::scene::kurbo;
use zgui::view::Ident;

use super::depth_cues;
use super::model::ViewportModel;

/// How a cast arc is drawn: its line, the line of the arc being edited, how far the dark outline
/// under a line reaches past each edge, and the plate and burst markers, in CSS pixels.
const ARC_LINE_WIDTH: f32 = 3.0;
const SELECTED_ARC_LINE_WIDTH: f32 = 5.0;
const ARC_OUTLINE: f32 = 1.5;
const IMPACT_RADIUS: f32 = 4.5;
const BURST_RADIUS: f32 = 7.0;

/// A few hardpoint markers are named outright; past that the names would bury the model, so only
/// the one under the pointer is.
const LABELLED_OUTRIGHT: usize = 16;

/// How a box label is set: its font size and the height of the plate behind it.
const LABEL_SIZE: f32 = 11.0;
const LABEL_LINE: f32 = 13.0;

/// The size of a gizmo axis letter, and how far above its ball centre the letter's box starts.
const AXIS_LABEL_SIZE: f32 = 10.0;

/// Shaped labels kept between paints, keyed by text and device size. Cleared rather than evicted
/// when it grows past this.
const SHAPED_CACHE_LIMIT: usize = 512;

/// The desktops' interface faces, tried in order, as the chart's labels are.
const UI_FAMILIES: [&str; 8] =
    ["Segoe UI", "Adwaita Sans", "Cantarell", "SF Pro Text", "Helvetica Neue", "Noto Sans", "DejaVu Sans", "Arial"];

/// A name written over the scene, centred above its anchor.
#[derive(Clone, Debug, PartialEq)]
pub(super) struct BoxLabel {
    pub(super) text: String,
    pub(super) at: Vec2,
    pub(super) opacity: f32,
}

/// One stretch of an arc's line in front of the camera.
#[derive(Clone, Debug, PartialEq)]
pub(super) struct ArcRun {
    pub(super) points: Vec<Vec2>,
    pub(super) opacity: f32,
}

/// One cast arc as it lands on the screen.
#[derive(Clone, Debug, PartialEq)]
pub(super) struct ArcPaint {
    pub(super) runs: Vec<ArcRun>,
    pub(super) width: f32,
    pub(super) color: [f32; 4],
    pub(super) impacts: Vec<(Vec2, [f32; 4])>,
    pub(super) detonation: Option<Vec2>,
}

/// A ring spreading from a hit.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) struct PulsePaint {
    pub(super) at: Vec2,
    pub(super) progress: f32,
}

/// One axis ball of the gizmo.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) struct GizmoBall {
    pub(super) axis: Axis,
    pub(super) positive: bool,
    pub(super) tip: Vec2,
}

/// The navigation gizmo: its centre and its balls, nearest the camera last.
#[derive(Clone, Debug, PartialEq)]
pub(super) struct GizmoPaint {
    pub(super) center: Vec2,
    pub(super) balls: Vec<GizmoBall>,
    pub(super) hover: Option<(Axis, bool)>,
}

/// Everything the overlay draws, in the viewport's CSS pixels.
#[derive(Clone, Debug, Default, PartialEq)]
pub(super) struct OverlayScene {
    pub(super) labels: Vec<BoxLabel>,
    pub(super) arcs: Vec<ArcPaint>,
    pub(super) pulses: Vec<PulsePaint>,
    pub(super) gizmo: Option<GizmoPaint>,
}

impl ViewportModel {
    /// What the overlay draws over the scene as it stands at `now`.
    pub(super) fn overlay_scene(&self, now: Instant) -> OverlayScene {
        let mut scene = OverlayScene::default();
        let Some(rect) = self.last_bounds else { return scene };
        let camera = &self.viewport.camera;

        if self.scene_is_shown() {
            let mut label = |text: &str, anchor: Vec3, opacity: f32| {
                if let Some(at) = camera.project_to_screen(anchor, rect).filter(|at| rect.contains(*at)) {
                    scene.labels.push(BoxLabel { text: text.to_owned(), at, opacity });
                }
            };
            for box_label in &self.splash_labels {
                label(&box_label.name, self.viewport.pos_to_world_space(Vec3::from(box_label.position)), 1.0);
            }

            // The grid is drawn in world space, so its labels are placed there too.
            if let Some(written) = self.length_marks.as_ref().filter(|_| self.display_settings.show_length_marks) {
                let eye = camera.eye_position();
                let marks = written.marks();
                let labels = written.labels(
                    |a, b| (a - eye).norm() < (b - eye).norm(),
                    |position| waterline_grid::grid_fade(marks, position),
                );
                for length in labels {
                    label(length.text, length.position, length.opacity);
                }
            }

            let hardpoints: Vec<_> = if self.hardpoint_labels.len() <= LABELLED_OUTRIGHT {
                self.hardpoint_labels.iter().collect()
            } else {
                self.hovered_hardpoint.and_then(|index| self.hardpoint_labels.get(index)).into_iter().collect()
            };
            for hardpoint in hardpoints {
                label(&hardpoint.name, self.viewport.pos_to_world_space(Vec3::from(hardpoint.position)), 1.0);
            }

            // The arc being edited is drawn last, over the others.
            let selected = self.selected_arc;
            let others = self.arcs.iter().filter(|arc| Some(arc.id) != selected);
            let editing = self.arcs.iter().filter(|arc| Some(arc.id) == selected);
            let project = |at: Vec3| camera.project_to_screen(at, rect);
            for arc in others.chain(editing) {
                scene.arcs.push(arc_paint(&arc.drawing, arc.color.rgba(), Some(arc.id) == selected, project));
            }

            for pulse in &self.hit_pulses {
                let anchor = self.viewport.pos_to_world_space(pulse.at);
                let Some(at) = camera.project_to_screen(anchor, rect).filter(|at| rect.contains(*at)) else {
                    continue;
                };
                let progress =
                    now.saturating_duration_since(pulse.started).as_secs_f32() / depth_cues::PULSE.as_secs_f32();
                scene.pulses.push(PulsePaint { at, progress });
            }
        }

        // The gizmo orients a model; with none loaded there is nothing for it to orient.
        if self.current_armor.is_some() && self.scene_problem.is_none() {
            let gizmo_rect = gizmo::gizmo_rect(rect);
            let center = gizmo_rect.center();
            let balls = gizmo::ball_draw_order(&camera.view_matrix())
                .into_iter()
                .map(|(axis, positive, direction, _depth)| GizmoBall {
                    axis,
                    positive,
                    tip: center + direction * gizmo::ARM_LEN,
                })
                .collect();
            scene.gizmo = Some(GizmoPaint { center, balls, hover: self.gizmo_hover });
        }

        scene
    }

    /// Writes the length marks' distances again when the language changed since they were
    /// written.
    pub(super) fn rewrite_length_marks(&mut self) {
        if let Some(written) = self.length_marks.as_ref().filter(|written| !written.is_current()) {
            self.length_marks = Some(MarkLabels::new(*written.marks()));
        }
    }
}

/// One cast shell as it lands on the screen: each line broken where it passes behind the camera,
/// a dot at each plate reached and a larger one where the fuse went off.
fn arc_paint(
    drawing: &trajectory::ArcDrawing,
    color: [f32; 4],
    selected: bool,
    project: impl Fn(Vec3) -> Option<Vec2>,
) -> ArcPaint {
    let mut runs = Vec::new();
    for line in &drawing.lines {
        let opacity = match line.stroke {
            trajectory::Stroke::Solid => 1.0,
            trajectory::Stroke::Faded => 0.45,
        };
        runs.extend(visible_runs(&line.points, &project).into_iter().map(|points| ArcRun { points, opacity }));
    }

    ArcPaint {
        runs,
        width: if selected { SELECTED_ARC_LINE_WIDTH } else { ARC_LINE_WIDTH },
        color,
        impacts: drawing.impacts.iter().filter_map(|impact| Some((project(impact.position)?, impact.color))).collect(),
        detonation: drawing.detonation.and_then(&project),
    }
}

/// The stretches of `points` in front of the camera that are long enough to draw. A point behind
/// the camera has no place on screen, so the line is broken there rather than joined across it.
fn visible_runs(points: &[Vec3], project: &impl Fn(Vec3) -> Option<Vec2>) -> Vec<Vec<Vec2>> {
    let mut runs = Vec::new();
    let mut run = Vec::new();
    for point in points {
        match project(*point) {
            Some(at) => run.push(at),
            None if run.len() >= 2 => runs.push(std::mem::take(&mut run)),
            None => run.clear(),
        }
    }

    if run.len() >= 2 {
        runs.push(run);
    }
    runs
}

/// A legend swatch's colour, each channel truncated to a byte as egui's
/// `Color32::from_rgba_unmultiplied` does.
pub(super) fn swatch_color(color: [f32; 4]) -> Color {
    let byte = |channel: f32| f32::from((channel * 255.0) as u8) / 255.0;
    Color::srgb(byte(color[0]), byte(color[1]), byte(color[2]), byte(color[3]))
}

fn axis_color(axis: Axis) -> Color {
    let (r, g, b): (u8, u8, u8) = match axis {
        Axis::X => (0xDC, 0x46, 0x46),
        Axis::Y => (0x5A, 0xC8, 0x5A),
        Axis::Z => (0x50, 0x82, 0xE6),
    };
    Color::srgb(f32::from(r) / 255.0, f32::from(g) / 255.0, f32::from(b) / 255.0, 1.0)
}

fn axis_label(axis: Axis) -> &'static str {
    match axis {
        Axis::X => "X",
        Axis::Y => "Y",
        Axis::Z => "Z",
    }
}

fn black(alpha: f32) -> Color {
    Color::srgb(0.0, 0.0, 0.0, alpha)
}

/// One label, shaped once: its runs, and its advance in device pixels.
struct Shaped {
    runs: Vec<ShapedRunOwned>,
    width: f32,
}

/// The overlay as a retained element.
pub(super) struct OverlayElement {
    pub(super) scene: OverlayScene,
    shaper: Shaper,
    families: Vec<Ident>,
    shaped: HashMap<(Arc<str>, u32), Shaped>,
}

impl OverlayElement {
    pub(super) fn new(shaper: Shaper) -> Self {
        Self {
            scene: OverlayScene::default(),
            shaper,
            families: UI_FAMILIES.iter().copied().map(Ident::new).collect(),
            shaped: HashMap::new(),
        }
    }
}

impl CustomElement for OverlayElement {
    fn layout(&mut self, cx: &mut CustomLayoutCx<'_>) -> CustomMeasured {
        // The box is sized by its stylesheet; the element has no size of its own to ask for.
        CustomMeasured {
            width: cx.known_width.unwrap_or(0.0),
            height: cx.known_height.unwrap_or(0.0),
            ..CustomMeasured::default()
        }
    }

    fn paint(&mut self, painter: &mut ScenePainter<'_>) {
        let scale = painter.scale();
        let Self { scene, shaper, families, shaped, .. } = self;
        if shaped.len() > SHAPED_CACHE_LIMIT {
            shaped.clear();
        }
        let mut canvas = Canvas { painter, scale, shaper, families, shaped };

        for label in &scene.labels {
            canvas.box_label(label);
        }
        for arc in &scene.arcs {
            canvas.arc(arc);
        }
        let hit = Color::srgb(
            wows_toolkit_armor::hits::COLOR[0],
            wows_toolkit_armor::hits::COLOR[1],
            wows_toolkit_armor::hits::COLOR[2],
            1.0,
        );
        for pulse in &scene.pulses {
            let (radius, opacity) = depth_cues::pulse_ring(pulse.progress);
            canvas.ring(pulse.at, radius, 1.5, hit.with_alpha(opacity));
        }
        if let Some(gizmo) = &scene.gizmo {
            canvas.gizmo(gizmo);
        }
    }
}

/// The painter, in CSS pixels scaled to the device.
struct Canvas<'a, 'p> {
    painter: &'a mut ScenePainter<'p>,
    scale: f32,
    shaper: &'a mut Shaper,
    families: &'a [Ident],
    shaped: &'a mut HashMap<(Arc<str>, u32), Shaped>,
}

impl Canvas<'_, '_> {
    fn rect(&self, x: f32, y: f32, width: f32, height: f32) -> Rect<DevicePx, Device> {
        Rect::new(
            Point::new(DevicePx(x * self.scale), DevicePx(y * self.scale)),
            Size::new(DevicePx(width * self.scale), DevicePx(height * self.scale)),
        )
    }

    /// A filled disc ringed in `ring`, `ring_width` wide.
    fn disc(&mut self, center: Vec2, radius: f32, fill: Color, ring: Option<(f32, Color)>) {
        let rect = self.rect(center.x - radius, center.y - radius, radius * 2.0, radius * 2.0);
        self.painter.fill(rect, radius * self.scale, fill);
        if let Some((width, color)) = ring {
            self.painter.stroke(rect, radius * self.scale, width * self.scale, color);
        }
    }

    /// A circle outline `width` wide.
    fn ring(&mut self, center: Vec2, radius: f32, width: f32, color: Color) {
        let rect = self.rect(center.x - radius, center.y - radius, radius * 2.0, radius * 2.0);
        self.painter.stroke(rect, radius * self.scale, width * self.scale, color);
    }

    fn polyline(&mut self, points: &[Vec2], width: f32, color: Color) {
        let Some((first, rest)) = points.split_first() else { return };
        let at = |point: &Vec2| kurbo::Point::new(f64::from(point.x * self.scale), f64::from(point.y * self.scale));
        let mut path = kurbo::BezPath::new();
        path.move_to(at(first));
        for point in rest {
            path.line_to(at(point));
        }
        let style = kurbo::Stroke::new(f64::from(width * self.scale))
            .with_join(kurbo::Join::Round)
            .with_caps(kurbo::Cap::Round);
        self.painter.shape(&Shape {
            path: Arc::new(path),
            fill: None,
            stroke: Some(Stroke { paint: Paint::Solid(Ink::Solid(color)), style }),
            clips: Vec::new(),
        });
    }

    /// `text` at `size` CSS pixels, shaped at the device size.
    fn shape(&mut self, text: &str, size: f32) -> (Arc<str>, u32) {
        let device_size = size * self.scale;
        let key = (Arc::<str>::from(text), device_size.to_bits());
        let Self { shaper, families, shaped, .. } = self;
        shaped.entry(key.clone()).or_insert_with(|| {
            let request = LineRequest {
                families,
                weight: 400,
                italic: false,
                size_device_px: device_size,
                letter_spacing: 0.0,
                ligatures: true,
            };
            // A shaped line reports where each glyph starts but not where the line ends, so one
            // more glyph is shaped after the text and its start taken as the text's advance.
            let mut runs = shaper.shape_line(&format!("{text}0"), &request);
            let width = match runs.last_mut() {
                Some(last) => {
                    let end = last.glyphs.pop().map_or(0.0, |glyph| glyph.x);
                    last.clusters.pop();
                    end
                }
                None => 0.0,
            };
            runs.retain(|run| !run.glyphs.is_empty());

            Shaped { runs, width }
        });
        key
    }

    /// The width of `text` at `size`, in CSS pixels.
    fn measure(&mut self, text: &str, size: f32) -> f32 {
        let key = self.shape(text, size);
        self.shaped.get(&key).map_or(0.0, |shaped| shaped.width / self.scale)
    }

    /// Writes `text` at `size` in a line box `line` tall whose top left is at `x`, `y`, centred
    /// vertically in it.
    fn text(&mut self, text: &str, size: f32, x: f32, y: f32, line: f32, color: Color) {
        let key = self.shape(text, size);
        let Some(shaped) = self.shaped.remove(&key) else { return };
        // The baseline sits where a line of this size centres its capitals in the box.
        let baseline = (y + line * 0.5 + size * 0.35) * self.scale;
        for run in &shaped.runs {
            let Some(first) = run.glyphs.first() else { continue };
            let line_top = baseline - first.y;
            self.painter.glyphs(
                &run.as_run(PaintSlot(0)),
                Point::new(DevicePx(x * self.scale), DevicePx(line_top)),
                color,
            );
        }
        self.shaped.insert(key, shaped);
    }

    /// A label centred above where it is anchored, on a dark plate so it reads against any hull,
    /// in the egui viewer's colours.
    fn box_label(&mut self, label: &BoxLabel) {
        let width = self.measure(&label.text, LABEL_SIZE);
        let (x, y) = (label.at.x - width * 0.5, label.at.y - LABEL_LINE - 2.0);
        let plate = self.rect(x - 2.0, y - 2.0, width + 4.0, LABEL_LINE + 4.0);
        self.painter.fill(plate, 2.0 * self.scale, black(f32::from(0x8c_u8) / 255.0 * label.opacity));
        let text = Color::srgb(
            f32::from(0xc8_u8) / 255.0,
            f32::from(0xdc_u8) / 255.0,
            1.0,
            f32::from(0xdc_u8) / 255.0 * label.opacity,
        );
        self.text(&label.text, LABEL_SIZE, x, y, LABEL_LINE, text);
    }

    /// One cast shell: each line over a dark outline so it reads against any hull, a ringed dot at
    /// each plate reached in its strike-angle colour, and a larger one in the arc's own colour
    /// where the fuse went off.
    fn arc(&mut self, arc: &ArcPaint) {
        let line_color = swatch_color(arc.color);
        for run in &arc.runs {
            self.polyline(&run.points, arc.width + ARC_OUTLINE * 2.0, black(0.8 * run.opacity));
            self.polyline(&run.points, arc.width, line_color.with_alpha(line_color.alpha() * run.opacity));
        }
        for (at, color) in &arc.impacts {
            self.disc(*at, IMPACT_RADIUS, swatch_color(*color), Some((1.5, black(1.0))));
        }
        if let Some(at) = arc.detonation {
            self.disc(at, BURST_RADIUS, line_color, Some((1.5, black(1.0))));
        }
    }

    /// The six axis balls, nearest the camera on top: a line to each positive tip with a filled
    /// ball and its letter, a ring for each negative one, and a white ring round the one hovered.
    fn gizmo(&mut self, gizmo: &GizmoPaint) {
        for ball in &gizmo.balls {
            let color = axis_color(ball.axis);
            if ball.positive {
                self.polyline(&[gizmo.center, ball.tip], 2.0, color);
                self.disc(ball.tip, gizmo::BALL_R, color, None);
                let label = axis_label(ball.axis);
                let width = self.measure(label, AXIS_LABEL_SIZE);
                self.text(label, AXIS_LABEL_SIZE, ball.tip.x - width * 0.5, ball.tip.y - 6.0, 12.0, black(1.0));
            } else {
                self.ring(ball.tip, gizmo::BALL_R, 1.5, color);
            }
            if gizmo.hover == Some((ball.axis, ball.positive)) {
                self.ring(ball.tip, gizmo::BALL_R + 2.0, 2.0, Color::srgb(1.0, 1.0, 1.0, 1.0));
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use wows_toolkit_armor::viewport::types::Vec2;
    use wows_toolkit_armor::viewport::types::Vec3;

    use super::visible_runs;

    #[test]
    fn a_line_is_broken_where_it_passes_behind_the_camera() {
        let points: Vec<Vec3> = (0..6).map(|index| Vec3::new(index as f32, 0.0, 0.0)).collect();
        let project = |at: Vec3| (at.x != 2.0).then(|| Vec2::new(at.x, 0.0));
        let runs = visible_runs(&points, &project);
        assert_eq!(runs.len(), 2);
        assert_eq!(runs[0].len(), 2);
        assert_eq!(runs[1].len(), 3);
    }

    #[test]
    fn a_lone_point_draws_nothing() {
        let points = [Vec3::new(0.0, 0.0, 0.0), Vec3::new(1.0, 0.0, 0.0), Vec3::new(2.0, 0.0, 0.0)];
        let project = |at: Vec3| (at.x == 0.0).then(|| Vec2::new(0.0, 0.0));
        assert!(visible_runs(&points, &project).is_empty());
    }
}

use tiny_skia::Color;
use tiny_skia::FillRule;
use tiny_skia::Paint;
use tiny_skia::PathBuilder;
use tiny_skia::Pixmap;
use tiny_skia::Rect;
use tiny_skia::Stroke;
use tiny_skia::Transform;

pub const FRAME_SIDE: u32 = 768;

const CIRCLES: usize = 1500;
const LINES: usize = 500;
const RECTS: usize = 100;

/// Pixel offset applied by drag input, in frame pixels.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct PanOffset {
    pub x: f32,
    pub y: f32,
}

#[derive(Debug, Clone, Copy)]
struct Mover {
    x: f32,
    y: f32,
    dx: f32,
    dy: f32,
    size: f32,
    rgba: [u8; 4],
}

/// Minimap-like load: moving circles and heading lines over static zones.
pub struct Scene {
    pixmap: Pixmap,
    circles: Vec<Mover>,
    lines: Vec<Mover>,
    rects: Vec<Mover>,
}

impl Scene {
    pub fn new() -> Self {
        let mut rng = XorShift(0x9E37_79B9_7F4A_7C15);
        let side = FRAME_SIDE as f32;
        let mut mover = |size_min: f32, size_span: f32, speed: f32| Mover {
            x: rng.unit() * side,
            y: rng.unit() * side,
            dx: (rng.unit() - 0.5) * speed,
            dy: (rng.unit() - 0.5) * speed,
            size: size_min + rng.unit() * size_span,
            rgba: [rng.byte(), rng.byte(), rng.byte(), 200],
        };

        let circles = (0..CIRCLES).map(|_| mover(2.0, 4.0, 3.0)).collect();
        let lines = (0..LINES).map(|_| mover(8.0, 16.0, 2.0)).collect();
        let rects = (0..RECTS).map(|_| mover(10.0, 30.0, 0.0)).collect();
        let pixmap = Pixmap::new(FRAME_SIDE, FRAME_SIDE).expect("frame side is nonzero");

        Self { pixmap, circles, lines, rects }
    }

    /// Advances one step and rasterizes; returns premultiplied RGBA.
    pub fn render(&mut self, pan: PanOffset) -> &[u8] {
        let side = FRAME_SIDE as f32;
        for mover in self.circles.iter_mut().chain(self.lines.iter_mut()) {
            mover.x = (mover.x + mover.dx).rem_euclid(side);
            mover.y = (mover.y + mover.dy).rem_euclid(side);
        }

        self.pixmap.fill(Color::from_rgba8(18, 32, 48, 255));
        let transform = Transform::from_translate(pan.x, pan.y);
        let mut paint = Paint { anti_alias: true, ..Paint::default() };

        for rect in &self.rects {
            paint.set_color_rgba8(rect.rgba[0], rect.rgba[1], rect.rgba[2], 70);
            if let Some(area) = Rect::from_xywh(rect.x, rect.y, rect.size * 2.0, rect.size) {
                self.pixmap.fill_rect(area, &paint, transform, None);
            }
        }

        for circle in &self.circles {
            paint.set_color_rgba8(circle.rgba[0], circle.rgba[1], circle.rgba[2], circle.rgba[3]);
            if let Some(path) = PathBuilder::from_circle(circle.x, circle.y, circle.size) {
                self.pixmap.fill_path(&path, &paint, FillRule::Winding, transform, None);
            }
        }

        let stroke = Stroke { width: 1.5, ..Stroke::default() };
        for line in &self.lines {
            paint.set_color_rgba8(line.rgba[0], line.rgba[1], line.rgba[2], 255);
            let length = line.size / (line.dx.hypot(line.dy).max(0.01));
            let mut builder = PathBuilder::new();
            builder.move_to(line.x, line.y);
            builder.line_to(line.x + line.dx * length, line.y + line.dy * length);
            if let Some(path) = builder.finish() {
                self.pixmap.stroke_path(&path, &paint, &stroke, transform, None);
            }
        }

        self.pixmap.data()
    }
}

struct XorShift(u64);

impl XorShift {
    fn next(&mut self) -> u64 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        self.0
    }

    fn unit(&mut self) -> f32 {
        (self.next() >> 40) as f32 / (1u64 << 24) as f32
    }

    fn byte(&mut self) -> u8 {
        (self.next() >> 56) as u8
    }
}

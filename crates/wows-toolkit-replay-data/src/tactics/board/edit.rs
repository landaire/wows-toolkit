//! Changing the one shape the reader has picked out alone: its ink, size and fill, or a placed
//! ship's side. Each change goes through the board's link, so a session sees it.

use wows_minimap_renderer::MINIMAP_SIZE;
use wt_collab_client::types::Annotation;

use crate::tactics::WORLD_UNITS_PER_KM;
use crate::tactics::board::Board;

/// What a circle's radius may be set to on a map whose measurements are known, in kilometres.
pub const MIN_SHAPE_KM: f32 = 0.1;
pub const MAX_SHAPE_KM: f32 = 20.0;

/// What a shape's size may be set to otherwise, in minimap units.
pub const MIN_SHAPE_UNITS: f32 = 5.0;
pub const MAX_SHAPE_UNITS: f32 = 500.0;

/// How big an outlined shape is: a circle's or triangle's radius, or a rectangle's mean half side.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum ShapeSize {
    /// A circle's radius on a map whose measurements are known.
    Km(f32),
    /// In minimap units: any other outlined shape, or a circle on a map with no measurements.
    MapUnits(f32),
}

/// The shape picked out alone, as its edit controls show it.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum PickedShape {
    /// A placed ship, by the side it is on.
    Ship { friendly: bool },
    /// A circle, rectangle or triangle.
    Outline { color: [u8; 4], size: ShapeSize, filled: bool },
    /// A freehand stroke, line, arrow or measurement.
    Stroke { color: [u8; 4] },
}

impl Board {
    /// The shape picked out alone. `None` with nothing or several picked.
    pub fn picked_shape(&self) -> Option<PickedShape> {
        let index = self.picked.single()?;
        let picked = match self.collab.annotation_at(index)? {
            Annotation::Ship { friendly, .. } => PickedShape::Ship { friendly },
            Annotation::Circle { radius, color, filled, .. } => {
                let size = match self.units_per_km() {
                    Some(per_km) => ShapeSize::Km(radius / per_km),
                    None => ShapeSize::MapUnits(radius),
                };
                PickedShape::Outline { color, size, filled }
            }
            Annotation::Rectangle { half_size, color, filled, .. } => {
                PickedShape::Outline { color, size: ShapeSize::MapUnits((half_size[0] + half_size[1]) / 2.0), filled }
            }
            Annotation::Triangle { radius, color, filled, .. } => {
                PickedShape::Outline { color, size: ShapeSize::MapUnits(radius), filled }
            }
            Annotation::FreehandStroke { color, .. }
            | Annotation::Line { color, .. }
            | Annotation::Arrow { color, .. }
            | Annotation::Measurement { color, .. } => PickedShape::Stroke { color },
        };

        Some(picked)
    }

    /// Draws the picked shape in `ink`. A ship has no ink of its own.
    pub fn set_picked_color(&mut self, ink: [u8; 4]) {
        self.change_picked(|annotation| match annotation {
            Annotation::Ship { .. } => {}
            Annotation::FreehandStroke { color, .. }
            | Annotation::Line { color, .. }
            | Annotation::Circle { color, .. }
            | Annotation::Rectangle { color, .. }
            | Annotation::Triangle { color, .. }
            | Annotation::Arrow { color, .. }
            | Annotation::Measurement { color, .. } => *color = ink,
        });
    }

    /// Resizes the picked outlined shape, within the range its size is shown in.
    pub fn set_picked_size(&mut self, size: ShapeSize) {
        let units = match size {
            ShapeSize::Km(km) => {
                let Some(per_km) = self.units_per_km() else { return };
                km.clamp(MIN_SHAPE_KM, MAX_SHAPE_KM) * per_km
            }
            ShapeSize::MapUnits(units) => units.clamp(MIN_SHAPE_UNITS, MAX_SHAPE_UNITS),
        };
        self.change_picked(|annotation| match annotation {
            Annotation::Circle { radius, .. } | Annotation::Triangle { radius, .. } => *radius = units,
            // Both sides by the same ratio, so the rectangle keeps its shape.
            Annotation::Rectangle { half_size, .. } => {
                let mean = (half_size[0] + half_size[1]) / 2.0;
                if mean > 0.0 {
                    let ratio = units / mean;
                    half_size[0] *= ratio;
                    half_size[1] *= ratio;
                }
            }
            _ => {}
        });
    }

    /// Fills or hollows the picked outlined shape.
    pub fn set_picked_filled(&mut self, on: bool) {
        self.change_picked(|annotation| match annotation {
            Annotation::Circle { filled, .. }
            | Annotation::Rectangle { filled, .. }
            | Annotation::Triangle { filled, .. } => *filled = on,
            _ => {}
        });
    }

    /// Puts the picked ship on the reader's side or the enemy's.
    pub fn set_picked_friendly(&mut self, on: bool) {
        self.change_picked(|annotation| {
            if let Annotation::Ship { friendly, .. } = annotation {
                *friendly = on;
            }
        });
    }

    /// Changes the shape picked out alone, as one step to take back, telling the session when it
    /// changed.
    fn change_picked(&mut self, change: impl FnOnce(&mut Annotation)) {
        if self.collab.annotations_locked() {
            return;
        }
        let Some(index) = self.picked.single() else { return };
        let Some(was) = self.collab.annotation_at(index) else { return };

        let mut changed = was.clone();
        change(&mut changed);
        if changed == was {
            return;
        }

        self.remember();
        self.collab.update_annotation(index, changed);
        self.redraw();
    }

    /// How many minimap units a kilometre is on the board's map. `None` where the map's
    /// measurements are not known.
    fn units_per_km(&self) -> Option<f32> {
        let space_size = self.map_info()?.space_size as f32;

        (space_size > 0.0).then(|| WORLD_UNITS_PER_KM * MINIMAP_SIZE as f32 / space_size)
    }
}

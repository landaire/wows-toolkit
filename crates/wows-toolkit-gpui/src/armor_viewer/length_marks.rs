//! Distances on the grid under a model: heavier lines at a round spacing,
//! counted along the length from the stern and across the beam from one
//! side, so a reader can see how long each part of the model is. Model space
//! is ship-model space, so [`ShipModelDistance`] reads it in metres.

use rust_i18n::t;
use wowsunpack::game_params::types::Meters;
use wowsunpack::game_params::types::ShipModelDistance;

use crate::viewport::types::Vec3;

/// The unit a distance is written in.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum LengthUnit {
    Meters,
    Centimeters,
}

impl LengthUnit {
    /// Centimetres below a metre, so a small part reads in whole numbers.
    fn for_scale(scale: Meters) -> Self {
        if scale.value() >= 1. { Self::Meters } else { Self::Centimeters }
    }

    fn per_metre(self) -> f32 {
        match self {
            Self::Meters => 1.,
            Self::Centimeters => 100.,
        }
    }
}

/// A distance as it is written: a number of `unit` to `decimals` places.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct Reading {
    pub(crate) value: f32,
    pub(crate) unit: LengthUnit,
    pub(crate) decimals: usize,
}

impl Reading {
    /// `distance` in the unit `scale` calls for, to as many places as a
    /// step of `scale` needs to read exactly.
    pub(crate) fn of(distance: Meters, scale: Meters) -> Self {
        let unit = LengthUnit::for_scale(scale);
        let step = scale.value() * unit.per_metre();
        let decimals = if step >= 1. { 0 } else { (-step.log10()).ceil() as usize };

        Self { value: distance.value() * unit.per_metre(), unit, decimals }
    }

    /// An overall size, to a tenth of its unit.
    pub(crate) fn of_size(size: Meters) -> Self {
        let unit = LengthUnit::for_scale(size);

        Self { value: size.value() * unit.per_metre(), unit, decimals: 1 }
    }

    pub(crate) fn number(&self) -> String {
        format!("{:.*}", self.decimals, self.value)
    }

    pub(crate) fn text(&self) -> String {
        let value = self.number();
        match self.unit {
            LengthUnit::Meters => t!("ui.armor.length_metres", value = value).into_owned(),
            LengthUnit::Centimeters => t!("ui.armor.length_centimetres", value = value).into_owned(),
        }
    }
}

/// One heavier line: where it crosses its axis in model space, and how far
/// that is from where the axis is counted from.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct Mark {
    pub(crate) at: f32,
    pub(crate) distance: Meters,
}

/// A distance written over the scene beside the grid, at `position` in
/// model space, as strong as the grid is there.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct LengthLabel {
    pub(crate) text: String,
    pub(crate) position: Vec3,
    pub(crate) opacity: f32,
}

/// The marks on the grid under a model with model-space `bounds`, on the
/// plane at height `plane_y`.
///
/// Along Z (the length of a ship, bow at -Z) they count from the +Z end, so
/// 0 is the stern and a mark reads as how far forward of it a part sits.
/// Across X they count from the -X side. One spacing serves both, so the
/// cells stay square.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct LengthMarks {
    low: Vec3,
    high: Vec3,
    plane_y: f32,
    spacing: Meters,
}

impl LengthMarks {
    /// `None` for bounds with no extent on the plane to measure.
    pub(crate) fn new(bounds: (Vec3, Vec3), plane_y: f32) -> Option<Self> {
        let (low, high) = bounds;
        let finite = [low.x, low.z, high.x, high.z, plane_y].into_iter().all(f32::is_finite);
        if !finite || high.x < low.x || high.z < low.z {
            return None;
        }

        let longest = meters((high.x - low.x).max(high.z - low.z));
        let spacing = mark_spacing(longest)?;

        Some(Self { low, high, plane_y, spacing })
    }

    pub(crate) fn spacing(&self) -> Meters {
        self.spacing
    }

    pub(crate) fn bounds(&self) -> (Vec3, Vec3) {
        (self.low, self.high)
    }

    pub(crate) fn plane_y(&self) -> f32 {
        self.plane_y
    }

    /// The size along Z, bow to stern on a ship.
    pub(crate) fn length(&self) -> Meters {
        meters(self.high.z - self.low.z)
    }

    /// The size across X.
    pub(crate) fn beam(&self) -> Meters {
        meters(self.high.x - self.low.x)
    }

    /// The marks along Z, from the stern forward.
    pub(crate) fn along_length(&self) -> Vec<Mark> {
        (0..=self.length_steps())
            .map(|step| self.step_distance(step))
            .map(|distance| Mark { at: self.high.z - distance.to_ship_model().value(), distance })
            .collect()
    }

    /// The marks across X, from the -X side.
    pub(crate) fn across_beam(&self) -> Vec<Mark> {
        (0..=self.beam_steps())
            .map(|step| self.step_distance(step))
            .map(|distance| Mark { at: self.low.x + distance.to_ship_model().value(), distance })
            .collect()
    }

    /// How many whole steps of the spacing fit along the length.
    pub(crate) fn length_steps(&self) -> u32 {
        self.steps(self.length())
    }

    /// How many whole steps of the spacing fit across the beam.
    pub(crate) fn beam_steps(&self) -> u32 {
        self.steps(self.beam())
    }

    fn steps(&self, extent: Meters) -> u32 {
        // A mark that falls on the far end by a rounding error still belongs.
        const SLACK: f32 = 1e-4;

        (extent.value() / self.spacing.value() + SLACK).floor() as u32
    }

    fn step_distance(&self, step: u32) -> Meters {
        Meters::new(step as f32 * self.spacing.value())
    }

    /// The marks labelled beside the grid on the sides nearer the camera:
    /// the length marks along the nearer side, the beam marks across the
    /// nearer end. `nearer(a, b)` says whether `a` on the plane is nearer the
    /// camera than `b`; `opacity` is how strong the grid is at a point.
    pub(crate) fn labels(
        &self,
        nearer: impl Fn(Vec3, Vec3) -> bool,
        opacity: impl Fn(Vec3) -> f32,
    ) -> Vec<LengthLabel> {
        // Clear of the hull, so a label is not written over the side.
        let margin = self.spacing.to_ship_model().value() * 0.4;
        let middle = (self.low + self.high) * 0.5;
        let at = |x: f32, z: f32| Vec3::new(x, self.plane_y, z);

        let side = |x: f32| at(x, middle.z);
        let (port, starboard) = (self.low.x - margin, self.high.x + margin);
        let near_x = if nearer(side(port), side(starboard)) { port } else { starboard };

        let end = |z: f32| at(middle.x, z);
        let (bow, stern) = (self.low.z - margin, self.high.z + margin);
        let near_z = if nearer(end(bow), end(stern)) { bow } else { stern };

        let label = |position: Vec3, distance: Meters| LengthLabel {
            text: Reading::of(distance, self.spacing).text(),
            position,
            opacity: opacity(position),
        };
        let along = self.along_length().into_iter().map(|mark| label(at(near_x, mark.at), mark.distance));
        let across = self.across_beam().into_iter().map(|mark| label(at(mark.at, near_z), mark.distance));

        along.chain(across).filter(|label| label.opacity > 0.).collect()
    }
}

/// The distance `units` of model space measure.
fn meters(units: f32) -> Meters {
    ShipModelDistance::from(units).to_meters()
}

/// The least of 1, 2 or 5 times a power of ten metres that cuts `longest`
/// into no more than 15 steps. The next spacing up is at most 2.5 times
/// larger, so there are always more than 6.
pub(crate) fn mark_spacing(longest: Meters) -> Option<Meters> {
    const MOST_STEPS: f32 = 15.;

    let longest = longest.value();
    if !longest.is_finite() || longest <= 0. {
        return None;
    }

    let power = 10_f32.powf((longest / MOST_STEPS).log10().floor());
    [1., 2., 5., 10.]
        .into_iter()
        .map(|leading| leading * power)
        .find(|spacing| longest / spacing <= MOST_STEPS)
        .map(Meters::new)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn bounds(length: Meters, beam: Meters) -> (Vec3, Vec3) {
        let (length, beam) = (length.to_ship_model().value(), beam.to_ship_model().value());
        (Vec3::new(-beam / 2., -0.3, -length / 2.), Vec3::new(beam / 2., 0.7, length / 2.))
    }

    fn spacing(metres: f32) -> f32 {
        mark_spacing(Meters::new(metres)).expect("a spacing").value()
    }

    /// A battleship, a destroyer, a small craft, a turret and a gun barrel
    /// each get a round spacing that cuts them into a countable number of
    /// steps.
    #[test]
    fn the_spacing_is_round_and_gives_a_countable_number_of_marks() {
        assert_eq!(spacing(270.4), 20.);
        assert_eq!(spacing(118.), 10.);
        assert_eq!(spacing(150.), 10.);
        assert_eq!(spacing(151.), 20.);
        assert_eq!(spacing(31.), 5.);
        assert!((spacing(12.) - 1.).abs() < 1e-6);
        assert!((spacing(0.8) - 0.1).abs() < 1e-6);
        assert!((spacing(0.07) - 0.005).abs() < 1e-6);

        for length in [0.07, 0.8, 3.3, 12., 31., 99., 118., 150., 151., 263., 270.4, 480., 2500.] {
            let steps = length / spacing(length);
            assert!(steps > 6. && steps <= 15. + 1e-3, "{length} m in {steps} steps");
        }
    }

    #[test]
    fn nothing_to_measure_has_no_spacing() {
        assert_eq!(mark_spacing(Meters::new(0.)), None);
        assert_eq!(mark_spacing(Meters::new(f32::NAN)), None);
        assert_eq!(LengthMarks::new((Vec3::zeros(), Vec3::new(0., 3., 0.)), 0.), None);
        let ship = (Vec3::new(-1., 0., -9.), Vec3::new(1., 2., 9.));
        assert_eq!(LengthMarks::new((Vec3::new(f32::NAN, 0., -9.), ship.1), 0.), None);
        assert_eq!(LengthMarks::new(ship, f32::INFINITY), None);
        assert_eq!(LengthMarks::new((ship.1, ship.0), 0.), None);
    }

    /// The length marks start at the stern (+Z) and step forward without
    /// passing the bow; the beam marks start at the -X side.
    #[test]
    fn marks_count_from_the_stern_and_stay_within_the_model() {
        let (low, high) = bounds(Meters::new(270.4), Meters::new(33.));
        let marks = LengthMarks::new((low, high), 0.).expect("marks");

        assert_eq!(marks.spacing(), Meters::new(20.));
        let along = marks.along_length();
        assert_eq!(along.len(), 14, "0 to 260 m");
        assert_eq!(along[0], Mark { at: high.z, distance: Meters::new(0.) });
        let forward = Meters::new(20.).to_ship_model().value();
        assert!((along[1].at - (high.z - forward)).abs() < 1e-4, "20 m forward of the stern: {:?}", along[1]);
        assert_eq!(along[13].distance, Meters::new(260.));
        assert!(along.iter().all(|mark| mark.at >= low.z && mark.at <= high.z));

        let across = marks.across_beam();
        assert_eq!(across.iter().map(|mark| mark.distance.value()).collect::<Vec<_>>(), vec![0., 20.]);
        assert_eq!(across[0].at, low.x);
    }

    #[test]
    fn a_mark_on_the_far_end_is_kept() {
        let marks = LengthMarks::new(bounds(Meters::new(100.), Meters::new(10.)), 0.).expect("marks");

        assert_eq!(marks.along_length().last().map(|mark| mark.distance), Some(Meters::new(100.)));
    }

    /// The labels sit on the plane, beside the hull on whichever side and
    /// end the camera is nearer, and take the grid's strength there.
    #[test]
    fn labels_sit_beside_the_model_on_the_side_facing_the_camera() {
        let (low, high) = bounds(Meters::new(200.), Meters::new(30.));
        let marks = LengthMarks::new((low, high), 0.).expect("marks");
        let eye = Vec3::new(500., 50., -400.);
        let nearer = |a: Vec3, b: Vec3| (a - eye).norm() < (b - eye).norm();

        let labels = marks.labels(nearer, |position| if position.z > 0. { 0.5 } else { 1. });

        assert_eq!(labels.len(), marks.along_length().len() + marks.across_beam().len());
        assert!(labels.iter().all(|label| label.position.y == 0.));
        let (along, across) = labels.split_at(marks.along_length().len());
        assert!(along.iter().all(|label| label.position.x > high.x), "on the +X side: {along:?}");
        assert!(across.iter().all(|label| label.position.z < low.z), "past the bow: {across:?}");
        assert_eq!(along[0].opacity, 0.5);
        assert_eq!(across[0].opacity, 1.);
    }

    #[test]
    fn labels_where_the_grid_has_faded_are_dropped() {
        let marks = LengthMarks::new(bounds(Meters::new(50.), Meters::new(10.)), 0.).expect("marks");

        assert!(marks.labels(|_, _| true, |_| 0.).is_empty());
    }

    /// Ships read in whole metres, small parts in centimetres, each to as
    /// many places as the spacing needs.
    #[test]
    fn readings_choose_a_unit_and_places_from_the_scale() {
        assert_eq!(
            Reading::of(Meters::new(260.), Meters::new(20.)),
            Reading { value: 260., unit: LengthUnit::Meters, decimals: 0 }
        );
        assert_eq!(Reading::of(Meters::new(0.3), Meters::new(0.1)).number(), "30");
        assert_eq!(Reading::of(Meters::new(0.3), Meters::new(0.1)).unit, LengthUnit::Centimeters);
        assert_eq!(Reading::of(Meters::new(0.015), Meters::new(0.005)).number(), "1.5");
        assert_eq!(Reading::of_size(Meters::new(270.43)).number(), "270.4");
        assert_eq!(Reading::of_size(Meters::new(0.456)).number(), "45.6");
    }

    /// Needs a local game install: Iowa's loaded hull, read as ship-model
    /// space, measures her real 270.4 m length and 33.0 m beam. Run the test
    /// binary with `WOWS_DIR=<install> <binary> --ignored iowa_measures`.
    #[test]
    #[ignore = "needs a game install named by WOWS_DIR"]
    fn iowa_measures_its_real_length() {
        use std::sync::Arc;

        use wowsunpack::export::ship::ShipAssets;
        use wowsunpack::game_data::AssetsBin;
        use wowsunpack::game_params::provider::GameMetadataProvider;

        let Some(dir) = std::env::var_os("WOWS_DIR").map(std::path::PathBuf::from) else {
            eprintln!("skipped: WOWS_DIR is not set");
            return;
        };
        let builds = wowsunpack::game_data::list_available_builds(&dir).expect("installed builds");
        let build = *builds.last().expect("at least one build");
        let vfs = wowsunpack::game_data::build_game_vfs_for_build(&dir, build, AssetsBin::Overlay).expect("game vfs");
        let metadata = Arc::new(GameMetadataProvider::from_vfs(&vfs).expect("game params"));
        let ship_assets = ShipAssets::from_vfs_with_metadata(&vfs, Arc::clone(&metadata)).expect("ship assets");
        let catalog = crate::armor_viewer::catalog::ShipCatalog::build(&metadata);
        let iowa = catalog
            .nations
            .iter()
            .flat_map(|nation| &nation.classes)
            .flat_map(|class| &class.ships)
            .find(|ship| ship.param_index == "PASB018")
            .expect("Iowa in the catalogue");
        let armor = crate::armor_viewer::load_ship::load_ship_armor_by_param(
            &ship_assets,
            &iowa.param_index,
            &iowa.display_name,
        )
        .expect("Iowa loads");

        let marks = LengthMarks::new(armor.bounds, 0.).expect("marks");
        eprintln!("Iowa bounds {:?}: {:?} long, {:?} wide", armor.bounds, marks.length(), marks.beam());
        let off_by = |measured: Meters, real: f32| (measured.value() - real).abs() / real;
        assert!(off_by(marks.length(), 270.4) < 0.03, "length {:?}", marks.length());
        // The boats and gun tubs reach past the hull's own 33.0 m beam.
        assert!(off_by(marks.beam(), 33.0) < 0.1, "beam {:?}", marks.beam());
    }
}

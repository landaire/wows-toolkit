//! Game art read from a build's game files: ship-class icons, nation flags and the keyed icons
//! (achievements, ribbons, modules, signals, captain skills, consumables), and the tone a class
//! icon is drawn in.
//!
//! The game's class icons are white and point up. Every view shows one turned to point right,
//! as GPUI does, so [`class_icon_svg`] is the only reader of them and turns them as it reads. A
//! row's icon is that SVG with every paint multiplied by the row's tone, as the game tints them,
//! either as markup or drawn to a PNG.

use std::cell::RefCell;
use std::collections::HashMap;
use std::collections::HashSet;
use std::io::Cursor;
use std::sync::Arc;

use image::ImageFormat;
use image::RgbaImage;
use landaire_ui::Rgb;
use resvg::tiny_skia::Pixmap;
use resvg::tiny_skia::Transform;
use resvg::usvg::Options;
use resvg::usvg::Tree;
use resvg::usvg::WriteOptions;
use wows_toolkit_replay_data::columns::PlayerColorKind;
use wows_toolkit_replay_data::load::LoadedGameData;
use wows_toolkit_viewmodel::player_tracker::live::PlayerTint;
use wowsunpack::game_assets::GuiAsset;
use wowsunpack::game_assets::ShipIconState;
use wowsunpack::game_params::types::CrewSkillName;
use wowsunpack::game_params::types::Species;
use wowsunpack::vfs::VfsPath;
use zgui::reactive::LocalStorage;
use zgui::reactive::RwSignal;
use zgui::reactive::StoredValue;
use zgui::reactive::prelude::Update;
use zgui::reactive::prelude::UpdateValue;
use zgui::reactive::prelude::With;
use zgui::reactive::prelude::WithUntracked;
use zgui::reactive::prelude::WithValue;
use zgui::task::Task;
use zgui::task::blocking;
use zgui::task::spawn_local;
use zgui_image::ImageBytes;

use crate::theme::Semantic;

/// The relation a class icon's tone shows.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ClassTone {
    SelfPlayer,
    Ally,
    Enemy,
    DivisionMate,
    Abuser,
}

impl ClassTone {
    /// The tone the icon is multiplied by: the dark theme's in either appearance. The tone
    /// multiplies the icon's white body, so a dark tone would leave the body the shade of its
    /// outline and the icon would read as a solid shape.
    pub fn rgb(self) -> Rgb {
        let tones = &Semantic::DARK;
        match self {
            Self::SelfPlayer => tones.text_strong,
            Self::Ally => tones.win,
            Self::Enemy => tones.loss,
            Self::DivisionMate => tones.division,
            Self::Abuser => tones.abuser,
        }
    }
}

impl From<PlayerColorKind> for ClassTone {
    fn from(kind: PlayerColorKind) -> Self {
        match kind {
            PlayerColorKind::SelfPlayer => Self::SelfPlayer,
            PlayerColorKind::Ally => Self::Ally,
            PlayerColorKind::Enemy => Self::Enemy,
            PlayerColorKind::DivisionMate => Self::DivisionMate,
            PlayerColorKind::Abuser => Self::Abuser,
        }
    }
}

impl From<PlayerTint> for ClassTone {
    fn from(tint: PlayerTint) -> Self {
        match tint {
            PlayerTint::SelfPlayer => Self::SelfPlayer,
            PlayerTint::Ally => Self::Ally,
            PlayerTint::Enemy => Self::Enemy,
            PlayerTint::DivisionMate => Self::DivisionMate,
            PlayerTint::Abuser => Self::Abuser,
        }
    }
}

/// `species`' white class icon in `vfs`, turned to point right, as SVG markup.
pub fn class_icon_svg(vfs: &VfsPath, species: Species) -> Result<String, ClassIconError> {
    let upright = GuiAsset::ShipClassIcon { species, state: ShipIconState::Alive }
        .read(vfs, None)
        .ok_or(ClassIconError::Missing(species))?;
    turned_svg(&upright, species)
}

/// `svg`, which points up, turned a quarter turn clockwise.
fn turned_svg(svg: &[u8], species: Species) -> Result<String, ClassIconError> {
    let tree = Tree::from_data(svg, &Options::default()).map_err(|source| ClassIconError::Parse { species, source })?;
    let (width, height) = (tree.size().width(), tree.size().height());
    // Written back by the parser, the icon has an explicit size, which the nested `svg` needs.
    let upright = tree.to_string(&WriteOptions::default());

    // (x, y) goes to (height - y, x).
    Ok(format!(
        "<svg xmlns=\"http://www.w3.org/2000/svg\" width=\"{height}\" height=\"{width}\" \
         viewBox=\"0 0 {height} {width}\"><g transform=\"matrix(0 1 -1 0 {height} 0)\">{upright}</g></svg>"
    ))
}

/// `nation`'s flag in `vfs`, as PNG bytes. `nation` is the nation's name in the game params.
pub fn nation_flag_png(vfs: &VfsPath, nation: &str) -> Option<Vec<u8>> {
    GuiAsset::NationFlag(nation).read(vfs, None)
}

/// `svg` with each `fill` and `stroke` colour multiplied by `tone`, channel by channel. Paints other
/// than a hex colour, white or black are left as they are.
pub fn tinted_svg(svg: &str, tone: Rgb) -> String {
    let mut out = String::with_capacity(svg.len());
    let mut rest = svg;
    while let Some(at) = rest.find("=\"") {
        let (head, tail) = rest.split_at(at + 2);
        out.push_str(head);
        let end = tail.find('"').unwrap_or(tail.len());
        let (value, after) = tail.split_at(end);
        let name = &head[..head.len() - 2];
        let paint = (name.ends_with(" fill") || name.ends_with(" stroke")).then(|| paint(value)).flatten();
        match paint {
            Some(color) => out.push_str(&multiply(color, tone).to_string()),
            None => out.push_str(value),
        }
        rest = after;
    }
    out.push_str(rest);

    out
}

fn paint(value: &str) -> Option<Rgb> {
    match value.trim() {
        "white" => Some(Rgb::new(0xffffff)),
        "black" => Some(Rgb::new(0x000000)),
        hex if hex.starts_with('#') => Rgb::parse(hex).ok(),
        _ => None,
    }
}

fn multiply(color: Rgb, tone: Rgb) -> Rgb {
    let channel = |a: u8, b: u8| ((u16::from(a) * u16::from(b) + 127) / 255) as u8;
    Rgb::from_channels(
        channel(color.red(), tone.red()),
        channel(color.green(), tone.green()),
        channel(color.blue(), tone.blue()),
    )
}

/// One class icon drawn in one tone.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct ClassIconKey {
    pub species: Species,
    pub tone: Rgb,
}

impl ClassIconKey {
    pub fn new(species: Species, tone: impl Into<ClassTone>) -> Self {
        Self { species, tone: tone.into().rgb() }
    }
}

#[derive(Debug, thiserror::Error)]
pub enum ClassIconError {
    #[error("the build has no icon for {0:?}")]
    Missing(Species),
    #[error("the icon for {species:?} is not a readable SVG")]
    Parse {
        species: Species,
        #[source]
        source: resvg::usvg::Error,
    },
    #[error("the icon for {species:?} has no area to draw")]
    Empty { species: Species },
    #[error("the icon for {species:?} could not be encoded")]
    Encode {
        species: Species,
        #[source]
        source: image::ImageError,
    },
}

/// `key`'s icon from `vfs`, drawn in its tone and turned to point right, as PNG bytes.
pub fn class_icon_png(vfs: &VfsPath, key: ClassIconKey) -> Result<Vec<u8>, ClassIconError> {
    let svg = class_icon_svg(vfs, key.species)?;
    tinted_png(&svg, key)
}

fn tinted_png(svg: &str, key: ClassIconKey) -> Result<Vec<u8>, ClassIconError> {
    let species = key.species;
    let tree = Tree::from_str(svg, &Options::default()).map_err(|source| ClassIconError::Parse { species, source })?;
    let size = tree.size().to_int_size();
    let mut pixmap = Pixmap::new(size.width(), size.height()).ok_or(ClassIconError::Empty { species })?;
    resvg::render(&tree, Transform::identity(), &mut pixmap.as_mut());

    let mut drawn = RgbaImage::new(size.width(), size.height());
    for (out, pixel) in drawn.pixels_mut().zip(pixmap.pixels()) {
        let color = pixel.demultiply();
        let tinted = multiply(Rgb::from_channels(color.red(), color.green(), color.blue()), key.tone);
        out.0 = [tinted.red(), tinted.green(), tinted.blue(), color.alpha()];
    }

    let mut png = Vec::new();
    drawn
        .write_to(&mut Cursor::new(&mut png), ImageFormat::Png)
        .map_err(|source| ClassIconError::Encode { species, source })?;
    Ok(png)
}

/// One picture a build holds.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum ArtKey {
    /// By the achievement's icon key.
    Achievement(String),
    /// By the ribbon's icon key.
    Ribbon(String),
    /// By the icon key of the ribbon it belongs to.
    SubRibbon(String),
    /// By the consumable's icon key.
    Consumable(String),
    /// By the module's game params name.
    Modernization(String),
    /// By the signal's game params name.
    SignalFlag(String),
    CrewSkill(CrewSkillName),
    /// By the nation's name in the game params.
    NationFlag(String),
    /// White and pointing right, for a caller to tint with [`tinted_svg`].
    ShipClass(Species),
}

impl ArtKey {
    fn read(&self, vfs: &VfsPath) -> Option<Art> {
        let raster = |asset: GuiAsset<'_>| asset.read(vfs, None).map(|bytes| Art::Raster(ImageBytes::new(bytes)));
        match self {
            Self::Achievement(key) => raster(GuiAsset::Achievement(key)),
            Self::Ribbon(key) => raster(GuiAsset::Ribbon(key)),
            Self::SubRibbon(key) => raster(GuiAsset::SubRibbon(&format!("sub{key}"))),
            Self::Consumable(key) => raster(GuiAsset::Consumable(key)),
            Self::Modernization(name) => raster(GuiAsset::Modernization(name)),
            Self::SignalFlag(name) => raster(GuiAsset::SignalFlag(name)),
            Self::CrewSkill(name) => raster(GuiAsset::CrewSkill { name }),
            Self::NationFlag(nation) => raster(GuiAsset::NationFlag(nation)),
            Self::ShipClass(species) => class_icon_svg(vfs, *species).ok().map(|svg| Art::Vector(Arc::from(svg))),
        }
    }
}

/// A picture as the window draws it.
#[derive(Clone, Debug)]
pub enum Art {
    /// PNG bytes, for an image element.
    Raster(ImageBytes),
    /// SVG markup, for a vector element.
    Vector(Arc<str>),
}

impl Art {
    /// The `src` of a raster picture.
    pub fn url(&self) -> Option<String> {
        match self {
            Self::Raster(bytes) => Some(bytes.url()),
            Self::Vector(_) => None,
        }
    }
}

/// The build a picture was read from. A newer build replaces it all.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct SourceBuild(u32);

/// What every view has read from the current build, the pictures it lacks included, so a picture
/// is read once per build however many views draw it.
#[derive(Default)]
struct ReadArt {
    source: Option<SourceBuild>,
    pictures: HashMap<ArtKey, Option<Art>>,
}

thread_local! {
    static READ: RefCell<ReadArt> = RefCell::new(ReadArt::default());
}

/// What `source` holds for `key`, when it has been read.
fn already_read(source: SourceBuild, key: &ArtKey) -> Option<Option<Art>> {
    READ.with_borrow(|read| if read.source == Some(source) { read.pictures.get(key).cloned() } else { None })
}

fn note_read(source: SourceBuild, pictures: &[(ArtKey, Option<Art>)]) {
    READ.with_borrow_mut(|read| {
        if read.source != Some(source) {
            *read = ReadArt { source: Some(source), pictures: HashMap::new() };
        }
        read.pictures.extend(pictures.iter().cloned());
    });
}

/// The pictures one view draws, as a `Copy` handle. Reads end with the view.
#[derive(Clone, Copy)]
pub struct GameArt {
    loaded: RwSignal<HashMap<ArtKey, Art>, LocalStorage>,
    work: StoredValue<ArtWork, LocalStorage>,
}

#[derive(Default)]
struct ArtWork {
    source: Option<SourceBuild>,
    asked: HashSet<ArtKey>,
    reads: Vec<Task>,
}

impl GameArt {
    pub fn new() -> Self {
        Self { loaded: RwSignal::new_local(HashMap::new()), work: StoredValue::new_local(ArtWork::default()) }
    }

    /// The picture under `key`, `None` until it is read or when the build has none. Reactive.
    pub fn get(self, key: &ArtKey) -> Option<Art> {
        self.loaded.with(|loaded| loaded.get(key).cloned())
    }

    /// Reads the pictures in `keys` not asked for yet from `data`. Without a build, drops them all.
    pub fn want(self, data: Option<&LoadedGameData>, keys: impl IntoIterator<Item = ArtKey>) {
        let Some(data) = data else {
            self.release();
            return;
        };

        let source = SourceBuild(data.build());
        if self.work.with_value(|work| work.source) != Some(source) {
            self.release();
            self.work.update_value(|work| work.source = Some(source));
        }

        let fresh: Vec<ArtKey> = self.work.with_value(|work| {
            let mut seen = HashSet::new();
            keys.into_iter().filter(|key| !work.asked.contains(key) && seen.insert(key.clone())).collect()
        });
        if fresh.is_empty() {
            return;
        }
        self.work.update_value(|work| work.asked.extend(fresh.iter().cloned()));

        let mut known = Vec::new();
        let mut unread = Vec::new();
        for key in fresh {
            match already_read(source, &key) {
                Some(Some(art)) => known.push((key, art)),
                Some(None) => {}
                None => unread.push(key),
            }
        }
        if !known.is_empty() {
            self.loaded.update(|loaded| loaded.extend(known));
        }
        if unread.is_empty() {
            return;
        }

        let vfs = data.vfs().clone();
        let read = spawn_local(async move {
            let read: Vec<(ArtKey, Option<Art>)> = blocking(move || {
                unread
                    .into_iter()
                    .map(|key| {
                        let art = key.read(&vfs);
                        (key, art)
                    })
                    .collect()
            })
            .await;
            note_read(source, &read);
            if self.work.try_with_value(|work| work.source) != Some(Some(source)) {
                return;
            }
            self.loaded.update(|loaded| loaded.extend(read.into_iter().filter_map(|(key, art)| Some((key, art?)))));
        });
        self.work.update_value(|work| {
            work.reads.retain(|read| !read.is_finished());
            work.reads.push(read);
        });
    }

    /// Holds `art` under `key` as though the build gave it.
    #[cfg(test)]
    pub fn insert_for_test(self, key: ArtKey, art: Art) {
        self.loaded.update(|loaded| {
            loaded.insert(key, art);
        });
    }

    fn release(self) {
        // A read of the build left behind must not refill the shared cache with it.
        let work = self.work.try_update_value(std::mem::take);
        for read in work.into_iter().flat_map(|work| work.reads) {
            read.cancel();
        }
        if self.loaded.with_untracked(|loaded| !loaded.is_empty()) {
            self.loaded.update(HashMap::clear);
        }
    }
}

impl Default for GameArt {
    fn default() -> Self {
        Self::new()
    }
}

/// A build holding a class icon drawn pointing up, as the game draws them, and the check that a
/// view shows it pointing right.
#[cfg(test)]
pub(crate) mod testing {
    use std::io::Write;

    use landaire_ui::HeadlessError;
    use landaire_ui::test_support::HeadlessWindow;
    use wowsunpack::game_assets::GuiAsset;
    use wowsunpack::game_assets::ShipIconState;
    use wowsunpack::game_params::types::Species;
    use wowsunpack::vfs::MemoryFS;
    use wowsunpack::vfs::VfsPath;

    /// White in the top half of its box only, so it points up. Sized by its view box alone.
    pub(crate) const UPRIGHT: &str = "<svg xmlns=\"http://www.w3.org/2000/svg\" viewBox=\"0 0 10 20\">\
        <rect x=\"0\" y=\"0\" width=\"10\" height=\"10\" fill=\"white\"/></svg>";

    /// A build whose only file is `species`' class icon, [`UPRIGHT`].
    pub(crate) fn upright_class_icon(species: Species) -> VfsPath {
        let vfs: VfsPath = MemoryFS::new().into();
        let path = GuiAsset::ShipClassIcon { species, state: ShipIconState::Alive }.candidate_paths(None).remove(0);
        let file = vfs.join(&path).expect("the icon's path is valid");
        file.parent().create_dir_all().expect("the icon's directory is made");
        file.create_file().expect("the icon is made").write_all(UPRIGHT.as_bytes()).expect("the icon is written");
        vfs
    }

    /// Whether `err` says the machine has no graphics device to draw on.
    pub(crate) fn no_device(err: &HeadlessError) -> bool {
        matches!(
            err,
            HeadlessError::NoWindow | HeadlessError::App { source: zgui::runtime::AppError::GpuUnavailable(_) }
        )
    }

    /// Panics unless the element with `class` shows [`UPRIGHT`] pointing right: lit towards its
    /// right edge and dark towards its left, along its middle.
    pub(crate) fn assert_points_right(window: &mut HeadlessWindow, class: &str) {
        let icon = window.by_class(class).unwrap_or_else(|| panic!("{class} is drawn"));
        let bounds = window.bounds(icon).unwrap_or_else(|| panic!("{class} has a box"));
        let pixels = window.pixels().expect("the window is drawn on the device");
        let middle_y = (bounds.origin.y.0 + bounds.size.height.0 / 2.0) as i32;
        let at = |fraction: f32| (bounds.origin.x.0 + bounds.size.width.0 * fraction) as i32;
        let brightness = |x: i32| pixels.rgba(x, middle_y)[..3].iter().map(|&channel| u32::from(channel)).sum::<u32>();
        let (point, base) = (at(0.8), at(0.2));

        assert!(brightness(point) > 300, "{class} has its point on the right: {:?}", pixels.rgba(point, middle_y));
        assert!(
            brightness(base) < brightness(point) / 3,
            "and its base on the left: {:?}",
            pixels.rgba(base, middle_y)
        );
    }
}

#[cfg(test)]
mod tests {
    use image::ImageFormat;
    use landaire_ui::Rgb;
    use resvg::tiny_skia::Pixmap;
    use resvg::tiny_skia::Transform;
    use resvg::usvg::Options;
    use resvg::usvg::Tree;
    use wows_toolkit_viewmodel::player_tracker::live::PlayerTint;
    use wowsunpack::game_params::types::Species;

    use super::ClassIconKey;
    use super::class_icon_png;
    use super::class_icon_svg;
    use super::testing::upright_class_icon;
    use super::tinted_svg;
    use crate::theme::Semantic;

    #[test]
    fn a_class_icon_is_tinted_as_the_game_tints_it() {
        let svg = r##"<svg viewBox="0 0 9 16" fill="none"><path fill-rule="evenodd" d="M9 16Z" fill="#333333"/><path d="M1 15Z" fill="white"/></svg>"##;

        assert_eq!(
            tinted_svg(svg, Rgb::new(0x80ff00)),
            r##"<svg viewBox="0 0 9 16" fill="none"><path fill-rule="evenodd" d="M9 16Z" fill="#1a3300"/><path d="M1 15Z" fill="#80ff00"/></svg>"##
        );
    }

    /// The tinted markup of `species`' icon drawn at its own size, with straight alpha.
    fn drawn_markup(species: Species, tone: Rgb) -> image::RgbaImage {
        let svg = class_icon_svg(&upright_class_icon(species), species).expect("the icon is read");
        let tree = Tree::from_str(&tinted_svg(&svg, tone), &Options::default()).expect("the markup parses");
        let size = tree.size().to_int_size();
        let mut pixmap = Pixmap::new(size.width(), size.height()).expect("the icon has an area");
        resvg::render(&tree, Transform::identity(), &mut pixmap.as_mut());
        let mut drawn = image::RgbaImage::new(size.width(), size.height());
        for (out, pixel) in drawn.pixels_mut().zip(pixmap.pixels()) {
            let color = pixel.demultiply();
            out.0 = [color.red(), color.green(), color.blue(), color.alpha()];
        }
        drawn
    }

    #[test]
    fn a_class_icon_read_as_markup_points_right_and_takes_a_tone() {
        let tone = Semantic::DARK.loss;
        let drawn = drawn_markup(Species::Battleship, tone);

        assert_eq!(drawn.dimensions(), (20, 10), "the upright 10x20 icon lies on its side");
        assert_eq!(drawn.get_pixel(15, 5).0, [tone.red(), tone.green(), tone.blue(), 255], "the point is on the right");
        assert_eq!(drawn.get_pixel(4, 5).0[3], 0, "the left half is empty");
    }

    #[test]
    fn a_class_icon_drawn_as_a_png_points_right_and_takes_the_rows_tone() {
        let key = ClassIconKey::new(Species::Battleship, PlayerTint::Ally);
        let png = class_icon_png(&upright_class_icon(Species::Battleship), key).expect("the icon draws");
        let decoded = image::load_from_memory_with_format(&png, ImageFormat::Png).expect("the PNG decodes").to_rgba8();

        assert_eq!(decoded.dimensions(), (20, 10));
        let tone = Semantic::DARK.win;
        assert_eq!(decoded.get_pixel(15, 5).0, [tone.red(), tone.green(), tone.blue(), 255]);
        assert_eq!(decoded.get_pixel(4, 5).0[3], 0);
    }
}

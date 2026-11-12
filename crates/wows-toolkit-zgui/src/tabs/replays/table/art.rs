//! Ship-class icons and nation flags, read from a build's game files.

use std::collections::HashMap;
use std::collections::HashSet;
use std::sync::Arc;

use landaire_ui::Rgb;
use wows_toolkit_replay_data::columns::PlayerColorKind;
use wows_toolkit_replay_data::model::PlayerRow;
use wowsunpack::game_assets::GuiAsset;
use wowsunpack::game_assets::ShipIconState;
use wowsunpack::game_params::types::Species;
use wowsunpack::vfs::VfsPath;
use zgui::reactive::LocalStorage;
use zgui::reactive::RwSignal;
use zgui::reactive::StoredValue;
use zgui::reactive::prelude::Set;
use zgui::reactive::prelude::SetValue;
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

/// A build's game files, which the table reads its icons and flags from.
#[derive(Clone)]
pub struct ArtSource {
    /// Which build `vfs` is, so a change of build drops what the last one gave.
    pub build: u32,
    pub vfs: VfsPath,
}

/// The build the art was read from. A newer build replaces it all.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct SourceBuild(u32);

/// What the build holds for the rows of one table.
#[derive(Clone, Default)]
struct Loaded {
    /// Each class's icon as SVG markup.
    classes: HashMap<Species, Arc<str>>,
    /// Each nation's flag as PNG bytes, by the nation's name in the game params.
    flags: HashMap<String, ImageBytes>,
}

struct Work {
    source: Option<SourceBuild>,
    asked_classes: HashSet<Species>,
    asked_flags: HashSet<String>,
    reads: Vec<Task>,
}

/// The art one table draws, as a `Copy` handle. Reads end with the table.
#[derive(Clone, Copy)]
pub(super) struct TableArt {
    loaded: RwSignal<Loaded, LocalStorage>,
    work: StoredValue<Work, LocalStorage>,
}

impl TableArt {
    pub(super) fn new() -> Self {
        Self {
            loaded: RwSignal::new_local(Loaded::default()),
            work: StoredValue::new_local(Work {
                source: None,
                asked_classes: HashSet::new(),
                asked_flags: HashSet::new(),
                reads: Vec::new(),
            }),
        }
    }

    /// `species`' icon tinted for `kind`, as SVG markup. Reactive.
    pub(super) fn class_icon(self, species: Species, kind: PlayerColorKind) -> Option<String> {
        self.loaded.with(|loaded| loaded.classes.get(&species).map(|svg| tinted(svg, class_icon_tint(kind))))
    }

    /// The `src` of `nation`'s flag. Reactive.
    pub(super) fn flag(self, nation: &str) -> Option<String> {
        self.loaded.with(|loaded| loaded.flags.get(nation).map(ImageBytes::url))
    }

    /// Reads the icons and flags `rows` need and are not yet asked for from `source`. Without a
    /// source, drops them all.
    pub(super) fn want(self, source: Option<ArtSource>, rows: &[PlayerRow]) {
        let Some(ArtSource { build, vfs }) = source else {
            self.release();
            return;
        };

        let source = SourceBuild(build);
        if self.work.with_value(|work| work.source) != Some(source) {
            self.release();
            self.work.update_value(|work| work.source = Some(source));
        }

        let (classes, flags) = self.work.with_value(|work| {
            let classes: HashSet<Species> =
                rows.iter().map(|row| row.ship_class).filter(|class| !work.asked_classes.contains(class)).collect();
            let flags: HashSet<String> = rows
                .iter()
                .filter_map(|row| row.ship_nation.clone())
                .filter(|nation| !work.asked_flags.contains(nation))
                .collect();
            (classes, flags)
        });
        if classes.is_empty() && flags.is_empty() {
            return;
        }
        self.work.update_value(|work| {
            work.asked_classes.extend(classes.iter().copied());
            work.asked_flags.extend(flags.iter().cloned());
        });

        let read = spawn_local(async move {
            let read = blocking(move || read_art(&vfs, classes, flags)).await;
            if self.work.try_with_value(|work| work.source) != Some(Some(source)) {
                return;
            }
            self.loaded.update(|loaded| {
                loaded.classes.extend(read.classes);
                loaded.flags.extend(read.flags);
            });
        });
        self.work.update_value(|work| work.reads.push(read));
    }

    fn release(self) {
        self.work.set_value(Work {
            source: None,
            asked_classes: HashSet::new(),
            asked_flags: HashSet::new(),
            reads: Vec::new(),
        });
        if self.loaded.with_untracked(|loaded| !loaded.classes.is_empty() || !loaded.flags.is_empty()) {
            self.loaded.set(Loaded::default());
        }
    }
}

/// The art in `vfs` for `classes` and `flags`. What the build lacks is left out.
fn read_art(vfs: &VfsPath, classes: HashSet<Species>, flags: HashSet<String>) -> Loaded {
    let classes = classes
        .into_iter()
        .filter_map(|species| {
            let bytes = GuiAsset::ShipClassIcon { species, state: ShipIconState::Alive }.read(vfs, None)?;
            Some((species, Arc::from(String::from_utf8(bytes).ok()?)))
        })
        .collect();
    let flags = flags
        .into_iter()
        .filter_map(|nation| {
            let bytes = GuiAsset::NationFlag(&nation).read(vfs, None)?;
            Some((nation, ImageBytes::new(bytes)))
        })
        .collect();

    Loaded { classes, flags }
}

/// The tint a ship-class icon takes for `kind`, in either theme: the dark theme's tones. The tint
/// multiplies the icon's white body, so a dark tone would leave the body the shade of its outline
/// and the icon would read as a solid shape.
fn class_icon_tint(kind: PlayerColorKind) -> Rgb {
    let tones = &Semantic::DARK;
    match kind {
        PlayerColorKind::SelfPlayer => tones.text_strong,
        PlayerColorKind::Ally => tones.win,
        PlayerColorKind::Enemy => tones.loss,
        PlayerColorKind::DivisionMate => tones.division,
        PlayerColorKind::Abuser => tones.abuser,
    }
}

/// `svg` with each `fill` and `stroke` colour multiplied by `tint`, channel by channel, as the game
/// tints its white class icons. Paints other than a hex colour, white or black are left as they are.
fn tinted(svg: &str, tint: Rgb) -> String {
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
            Some(color) => out.push_str(&multiply(color, tint).to_string()),
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

fn multiply(color: Rgb, tint: Rgb) -> Rgb {
    let channel = |a: u8, b: u8| ((u16::from(a) * u16::from(b) + 127) / 255) as u8;
    Rgb::from_channels(
        channel(color.red(), tint.red()),
        channel(color.green(), tint.green()),
        channel(color.blue(), tint.blue()),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_class_icon_is_tinted_as_the_game_tints_it() {
        let svg = r##"<svg viewBox="0 0 9 16" fill="none"><path fill-rule="evenodd" d="M9 16Z" fill="#333333"/><path d="M1 15Z" fill="white"/></svg>"##;

        assert_eq!(
            tinted(svg, Rgb::new(0x80ff00)),
            r##"<svg viewBox="0 0 9 16" fill="none"><path fill-rule="evenodd" d="M9 16Z" fill="#1a3300"/><path d="M1 15Z" fill="#80ff00"/></svg>"##
        );
    }
}

//! The small map a replay row leads with, one per map, read from the newest build open.

use std::collections::HashMap;
use std::collections::HashSet;
use std::io::Cursor;

use image::RgbImage;
use wows_toolkit_replay_data::load::GameDataCache;
use zgui::reactive::LocalStorage;
use zgui::reactive::Owner;
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

/// Pixels on each edge, twice the drawn edge so the thumbnail stays sharp at a scale of two.
const THUMBNAIL_PIXELS: u32 = 64;

/// The build the thumbnails were read from. A newer build replaces them all.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct SourceBuild(u32);

/// One map's thumbnail.
#[derive(Clone, Debug)]
pub enum Thumbnail {
    Loading,
    Ready(ImageBytes),
    /// The build has no art for the map.
    Missing,
}

/// The thumbnails of one listing, as a `Copy` handle.
#[derive(Clone, Copy)]
pub struct Thumbnails {
    slots: RwSignal<HashMap<String, Thumbnail>, LocalStorage>,
    work: StoredValue<ThumbnailWork, LocalStorage>,
}

struct ThumbnailWork {
    owner: Owner,
    source: Option<SourceBuild>,
    /// Each load reads the spaces it was handed, one after another.
    loading: Vec<Task>,
}

impl Thumbnails {
    /// Called from a component's body.
    pub fn new() -> Self {
        Self {
            slots: RwSignal::new_local(HashMap::new()),
            work: StoredValue::new_local(ThumbnailWork {
                owner: Owner::current().expect("thumbnails are made inside a component"),
                source: None,
                loading: Vec::new(),
            }),
        }
    }

    /// The thumbnail of `space`, or `None` while none is wanted. Reactive.
    pub fn get(self, space: &str) -> Option<Thumbnail> {
        self.slots.with(|slots| slots.get(space).cloned())
    }

    /// Loads the thumbnails of `spaces` not held yet, from the newest build `cache` has open.
    /// With no build open, drops them all.
    pub fn want(self, cache: Option<&GameDataCache>, spaces: HashSet<String>) {
        let newest = cache.and_then(GameDataCache::newest_loaded);
        let Some((cache, newest)) = cache.cloned().zip(newest) else {
            self.release();
            return;
        };
        let source = SourceBuild(newest.build());
        if self.work.with_value(|work| work.source) != Some(source) {
            self.release();
            self.work.update_value(|work| work.source = Some(source));
        }

        let fresh: Vec<String> =
            self.slots.with_untracked(|slots| spaces.into_iter().filter(|space| !slots.contains_key(space)).collect());
        if fresh.is_empty() {
            return;
        }
        self.slots.update(|slots| {
            for space in &fresh {
                slots.insert(space.clone(), Thumbnail::Loading);
            }
        });

        let owner = self.work.with_value(|work| work.owner.clone());
        let task = owner.with(|| {
            spawn_local(async move {
                for space in fresh {
                    let cache = cache.clone();
                    let reading = space.clone();
                    let image = blocking(move || load(&cache, source, &reading)).await;
                    if self.work.try_with_value(|work| work.source) != Some(Some(source)) {
                        return;
                    }
                    let thumbnail = match image {
                        Some(bytes) => Thumbnail::Ready(ImageBytes::new(bytes)),
                        None => Thumbnail::Missing,
                    };
                    self.slots.update(|slots| {
                        slots.insert(space, thumbnail);
                    });
                }
            })
        });
        self.work.update_value(|work| {
            work.loading.retain(|task| !task.is_finished());
            work.loading.push(task);
        });
    }

    fn release(self) {
        self.work.update_value(|work| {
            work.source = None;
            for task in work.loading.drain(..) {
                task.cancel();
            }
        });
        if self.slots.with_untracked(|slots| !slots.is_empty()) {
            self.slots.update(HashMap::clear);
        }
    }
}

/// `space`'s art from `source`, scaled and encoded as PNG for the image element.
fn load(cache: &GameDataCache, source: SourceBuild, space: &str) -> Option<Vec<u8>> {
    let loaded = cache.loaded_build(source.0)?;
    let art = wows_minimap_renderer::assets::load_map_image(space, loaded.vfs())?;

    encode(&art)
}

fn encode(art: &RgbImage) -> Option<Vec<u8>> {
    let scaled =
        image::imageops::resize(art, THUMBNAIL_PIXELS, THUMBNAIL_PIXELS, image::imageops::FilterType::Triangle);
    let mut bytes = Vec::new();
    match scaled.write_to(&mut Cursor::new(&mut bytes), image::ImageFormat::Png) {
        Ok(()) => Some(bytes),
        Err(err) => {
            tracing::warn!(error = %err, "a map thumbnail could not be encoded");
            None
        }
    }
}

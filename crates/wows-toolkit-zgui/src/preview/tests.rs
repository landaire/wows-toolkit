use std::cell::Cell;
use std::cell::RefCell;
use std::path::Path;
use std::path::PathBuf;
use std::rc::Rc;
use std::sync::Arc;
use std::sync::atomic::AtomicBool;
use std::sync::atomic::Ordering;
use std::time::Duration;

use futures::FutureExt as _;
use futures::channel::mpsc;
use futures::channel::oneshot;
use image::Rgb;
use image::RgbImage;
use landaire_ui::HeadlessError;
use landaire_ui::Scheme;
use landaire_ui::ThemeChoice;
use landaire_ui::ThemeRootProps;
use landaire_ui::WorkspaceInsetProps;
use landaire_ui::test_support::Drawing;
use landaire_ui::test_support::HeadlessOptions;
use landaire_ui::test_support::HeadlessWindow;
use landaire_ui::test_support::WindowSize;
use wows_toolkit_replay_data::frame::BgraFrame;
use wows_toolkit_replay_data::minimap_preview::FRAME_INTERVAL;
use wows_toolkit_replay_data::minimap_preview::PreviewError;
use wows_toolkit_replay_data::minimap_preview::PreviewFrames;
use wows_toolkit_viewmodel::preview_dwell::DWELL;
use zgui::component;
use zgui::reactive::Signal;
use zgui::view;
use zgui::view::IntoView;
use zgui::view::NodeRef;

use crate::frames::FrameSink;
use crate::frames::FrameSize;
use crate::frames::FrameViewProps;
use crate::preview::HoverPreview;
use crate::preview::MapArea;
use crate::preview::PreviewTarget;
use crate::preview::bake::BakeRequest;
use crate::preview::bake::BakeStreams;
use crate::preview::bake::PreviewBaker;
use crate::preview::card::HoverPreviewCardProps;

/// A baker whose bakes the test drives by hand.
#[derive(Default)]
struct ScriptedBaker {
    unavailable: Cell<bool>,
    walked: RefCell<Vec<PathBuf>>,
    bakes: RefCell<Vec<Scripted>>,
}

/// One bake as the test sees it: what it was asked for, and the ends it answers through.
struct Scripted {
    path: PathBuf,
    cancel: Arc<AtomicBool>,
    own_map: Option<oneshot::Sender<PreviewFrames>>,
    frames: mpsc::UnboundedSender<BgraFrame>,
    finished: Option<oneshot::Sender<Result<(), PreviewError>>>,
}

impl ScriptedBaker {
    fn send_map(&self, bake: usize, map: BgraFrame) {
        let mut frames = PreviewFrames::streaming();
        frames.push(map);
        frames.finish();
        let sender = self.bakes.borrow_mut()[bake].own_map.take().expect("the map is sent once");
        sender.send(frames).ok().expect("the bake waits for its map");
    }

    /// Ends the bake with `result`, with no map if none was sent.
    fn finish(&self, bake: usize, result: Result<(), PreviewError>) {
        let mut bakes = self.bakes.borrow_mut();
        bakes[bake].own_map = None;
        bakes[bake].frames.close_channel();
        let sender = bakes[bake].finished.take().expect("a bake finishes once");
        sender.send(result).expect("the bake waits to finish");
    }

    fn send_frame(&self, bake: usize, frame: BgraFrame) -> bool {
        self.bakes.borrow()[bake].frames.unbounded_send(frame).is_ok()
    }

    fn paths(&self) -> Vec<PathBuf> {
        self.bakes.borrow().iter().map(|bake| bake.path.clone()).collect()
    }
}

impl PreviewBaker for ScriptedBaker {
    fn is_available(&self) -> bool {
        !self.unavailable.get()
    }

    fn is_walked(&self, path: &Path) -> bool {
        self.walked.borrow().iter().any(|walked| walked == path)
    }

    fn start(&self, request: BakeRequest) -> Option<BakeStreams> {
        let (map_sender, own_map) = oneshot::channel();
        let (frame_sender, frames) = mpsc::unbounded();
        let (finished_sender, finished) = oneshot::channel();
        self.bakes.borrow_mut().push(Scripted {
            path: request.path,
            cancel: request.cancel,
            own_map: Some(map_sender),
            frames: frame_sender,
            finished: Some(finished_sender),
        });

        Some(BakeStreams {
            open_map: futures::future::ready(None).boxed_local(),
            own_map,
            frames,
            finished: async move { finished.await.unwrap_or(Err(PreviewError::Cancelled)) }.boxed_local(),
        })
    }
}

#[derive(Clone, Copy)]
struct Handles {
    preview: HoverPreview,
    first: NodeRef,
    second: NodeRef,
}

impl Handles {
    fn enter(self, name: &str, anchor: NodeRef) {
        self.preview.enter(PreviewTarget { path: PathBuf::from(name), map_name: None, anchor });
    }

    fn presented(self) -> Option<usize> {
        self.preview.with_run(|run| run.track.as_ref().and_then(|track| track.presented)).flatten()
    }

    fn sink(self) -> FrameSink {
        self.preview.sink().expect("the rows are mounted")
    }
}

/// Two rows with a preview over them.
#[component]
fn Rows(baker: Rc<ScriptedBaker>, handles: Rc<Cell<Option<Handles>>>) -> impl IntoView {
    let preview = HoverPreview::with_baker(baker);
    let first = NodeRef::new();
    let second = NodeRef::new();
    handles.set(Some(Handles { preview, first, second }));

    view! {
        column(style:width = "240px") {
            box(node_ref = first, attr:data-testid = "row-a", style:height = "24px") {"a.wowsreplay"}
            box(node_ref = second, attr:data-testid = "row-b", style:height = "24px") {"b.wowsreplay"}
            HoverPreviewCard(preview = preview) {
                text {"facts"}
            }
        }
    }
}

fn open_rows() -> (HeadlessWindow, Rc<ScriptedBaker>, Handles) {
    let baker = Rc::new(ScriptedBaker::default());
    let handles = Rc::new(Cell::new(None));
    let (mounted_baker, mounted_handles) = (Rc::clone(&baker), Rc::clone(&handles));
    let window = HeadlessWindow::open(HeadlessOptions::new(WindowSize { width: 900.0, height: 600.0 }), move || {
        let (baker, handles) = (Rc::clone(&mounted_baker), Rc::clone(&mounted_handles));
        view! { Rows(baker = baker, handles = handles) }
    })
    .expect("the rows open headlessly");
    let handles = handles.get().expect("the rows were built");

    (window, baker, handles)
}

fn solid(shade: u8) -> BgraFrame {
    BgraFrame::from_rgb(&RgbImage::from_pixel(8, 8, Rgb([shade, shade, shade])))
}

fn ms(millis: u64) -> Duration {
    Duration::from_millis(millis)
}

#[test]
fn the_bake_waits_for_the_pointer_to_settle() {
    let (mut window, baker, handles) = open_rows();

    handles.enter("a.wowsreplay", handles.first);
    window.advance(DWELL - ms(50));
    assert!(baker.paths().is_empty(), "the pointer has not settled yet");
    assert_eq!(handles.preview.map_area(), MapArea::Holding);
    assert!(window.by_test_id("hover-preview-water").is_some(), "the map's space is held while the pointer settles");
    assert!(window.by_class("zui-spinner").is_some(), "under a spinner from the start");

    window.advance(ms(60));
    assert_eq!(baker.paths(), [PathBuf::from("a.wowsreplay")]);
    assert_eq!(handles.preview.map_area(), MapArea::Holding);

    let card = window.by_test_id("hover-preview").expect("the card is up");
    let row = window.by_test_id("row-a").expect("the row is drawn");
    let (card, row) =
        (window.bounds(card).expect("the card has a box"), window.bounds(row).expect("the row has a box"));
    assert!(
        card.origin.x.0 >= row.origin.x.0 + row.size.width.0,
        "the card sits beside the row: {card:?} against {row:?}"
    );
}

#[test]
fn a_battle_walked_recently_bakes_without_the_dwell() {
    let (mut window, baker, handles) = open_rows();
    baker.walked.borrow_mut().push(PathBuf::from("a.wowsreplay"));

    handles.enter("a.wowsreplay", handles.first);
    window.advance(ms(16));

    assert_eq!(baker.paths(), [PathBuf::from("a.wowsreplay")]);
}

#[test]
fn moving_to_another_row_cancels_the_bake() {
    let (mut window, baker, handles) = open_rows();
    baker.walked.borrow_mut().push(PathBuf::from("a.wowsreplay"));
    handles.enter("a.wowsreplay", handles.first);
    window.advance(ms(16));
    baker.send_map(0, solid(10));
    window.advance(ms(16));
    assert_eq!(handles.preview.map_area(), MapArea::Frames { waiting: true }, "the still map waits for the track");
    assert_eq!(handles.sink().frame_size(), Some(FrameSize { width: 8, height: 8 }));

    handles.enter("b.wowsreplay", handles.second);
    window.advance(ms(16));

    assert!(baker.bakes.borrow()[0].cancel.load(Ordering::Relaxed), "the first row's bake is told to stop");
    assert!(!baker.send_frame(0, solid(20)), "and nothing is listening for its frames");
    assert_eq!(handles.preview.watched_untracked(), Some(PathBuf::from("b.wowsreplay")));
    assert_eq!(handles.preview.map_area(), MapArea::Holding, "the second row waits out its dwell");
    assert_eq!(handles.sink().frame_size(), None, "the first row's map comes down");

    window.advance(DWELL + ms(20));
    assert_eq!(baker.paths(), [PathBuf::from("a.wowsreplay"), PathBuf::from("b.wowsreplay")]);
}

#[test]
fn leaving_the_rows_ends_the_preview() {
    let (mut window, baker, handles) = open_rows();
    baker.walked.borrow_mut().push(PathBuf::from("a.wowsreplay"));
    handles.enter("a.wowsreplay", handles.first);
    window.advance(ms(16));
    assert!(window.by_test_id("hover-preview").is_some());

    handles.preview.leave();
    window.advance(ms(16));

    assert!(window.by_test_id("hover-preview").is_none(), "the card comes down");
    assert!(baker.bakes.borrow()[0].cancel.load(Ordering::Relaxed), "and the bake is told to stop");
}

#[test]
fn the_track_plays_and_loops_once_it_is_whole() {
    let (mut window, baker, handles) = open_rows();
    baker.walked.borrow_mut().push(PathBuf::from("a.wowsreplay"));
    handles.enter("a.wowsreplay", handles.first);
    window.advance(ms(16));
    baker.send_map(0, solid(10));
    for shade in [1, 2, 3] {
        assert!(baker.send_frame(0, solid(shade)));
    }
    window.advance(ms(16));
    assert_eq!(handles.preview.map_area(), MapArea::Frames { waiting: false }, "the track replaces the still map");
    assert_eq!(handles.presented(), Some(0));

    let mut played = Vec::new();
    for _ in 0..4 {
        window.advance(FRAME_INTERVAL);
        played.extend(handles.presented());
    }
    assert_eq!(played.first(), Some(&1), "playback moves on at the frame interval: {played:?}");
    assert_eq!(played.last(), Some(&2), "an unfinished track holds on its newest frame: {played:?}");

    baker.finish(0, Ok(()));
    let mut looped = Vec::new();
    for _ in 0..4 {
        window.advance(FRAME_INTERVAL);
        looped.extend(handles.presented());
    }
    assert!(looped.contains(&0), "a whole track starts over: {looped:?}");
}

#[test]
fn a_failed_bake_says_why_in_place_of_the_map() {
    let (mut window, baker, handles) = open_rows();
    baker.walked.borrow_mut().push(PathBuf::from("a.wowsreplay"));
    handles.enter("a.wowsreplay", handles.first);
    window.advance(ms(16));

    baker.finish(0, Err(PreviewError::NoMapInfo { map: "spaces/99_atlantis".to_owned() }));
    window.advance(ms(16));

    let MapArea::Failed(reason) = handles.preview.map_area() else {
        panic!("the bake failed, so the card says why: {:?}", handles.preview.map_area());
    };
    assert!(reason.contains("99_atlantis"), "the reason names what was missing: {reason}");
    let failed = window.by_test_id("hover-preview-failed").expect("the reason is where the map would be");
    assert_eq!(window.a11y(failed).and_then(|state| state.label), Some(reason));
}

#[test]
fn without_game_data_the_card_holds_no_map() {
    let (mut window, baker, handles) = open_rows();
    baker.unavailable.set(true);

    handles.enter("a.wowsreplay", handles.first);
    window.advance(DWELL + ms(50));

    assert!(baker.paths().is_empty(), "there is nothing to bake against");
    assert_eq!(handles.preview.map_area(), MapArea::Absent);
    assert!(window.by_test_id("hover-preview").is_some(), "the row's facts still go up");
    assert!(window.by_test_id("hover-preview-water").is_none(), "with no space held for a map");
}

/// A frame of `width` by `height` drawn by `paint`.
fn drawn(width: u32, height: u32, paint: impl Fn(u32, u32) -> [u8; 3]) -> BgraFrame {
    BgraFrame::from_rgb(&RgbImage::from_fn(width, height, |x, y| Rgb(paint(x, y))))
}

/// Colour bars over a grey ramp, four by three.
fn test_card() -> BgraFrame {
    const BARS: [[u8; 3]; 7] =
        [[192, 192, 192], [192, 192, 0], [0, 192, 192], [0, 192, 0], [192, 0, 192], [192, 0, 0], [0, 0, 192]];

    drawn(400, 300, |x, y| {
        if y < 220 {
            BARS[(x * 7 / 400) as usize]
        } else {
            let level = (x * 255 / 399) as u8;
            [level, level, level]
        }
    })
}

/// Water with a grid, two islands, and at `step` a ship of each team on its way across.
fn sea(step: u32) -> BgraFrame {
    let island = |x: u32, y: u32, cx: u32, cy: u32, r: u32| x.abs_diff(cx).pow(2) + y.abs_diff(cy).pow(2) < r * r;
    let ship = |x: u32, y: u32, cx: u32, cy: u32| x.abs_diff(cx) < 4 && y.abs_diff(cy) < 4;

    drawn(384, 384, move |x, y| {
        if ship(x, y, 60 + step * 12, 300 - step * 8) {
            return [80, 220, 120];
        }
        if ship(x, y, 320 - step * 12, 80 + step * 8) {
            return [240, 90, 90];
        }
        if island(x, y, 140, 150, 34) || island(x, y, 260, 250, 26) {
            return [142, 132, 96];
        }
        if x % 38 == 0 || y % 38 == 0 {
            return [24, 70, 82];
        }
        [16, 56, 68]
    })
}

/// The frame view beside two rows, one of them previewed, as the replay listing shows them.
#[component]
fn Gallery(baker: Rc<ScriptedBaker>, handles: Rc<Cell<Option<Handles>>>, sink: FrameSink) -> impl IntoView {
    let preview = HoverPreview::with_baker(baker);
    let first = NodeRef::new();
    let second = NodeRef::new();
    handles.set(Some(Handles { preview, first, second }));
    let choice = Signal::stored_local(ThemeChoice::Fixed(Scheme::Dark));

    view! {
        ThemeRoot(choice = choice) {
            WorkspaceInset {
                column(style:padding = "16px", style:gap = "12px") {
                    text {"Frame view: a 4:3 frame in a 16:9 box"}
                    box(style:width = "320px", style:height = "180px", style:border = "1px solid #555555") {
                        FrameView(sink = sink)
                    }
                    box(node_ref = first, style:padding = "6px 8px", style:width = "300px") {"20260611_ranked_Yamato_Atlantis.wowsreplay"}
                    box(node_ref = second, style:padding = "6px 8px", style:width = "300px") {"20260612_random_Shimakaze_Ocean.wowsreplay"}
                    HoverPreviewCard(preview = preview) {
                        column(style:font-size = "12px", style:padding = "2px 4px") {
                            text {"Ship: Yamato"}
                            text {"Damage: 154,320"}
                        }
                    }
                }
            }
        }
    }
}

/// Opens the gallery on the graphics device, or `None` with a note when there is no adapter.
fn open_gallery(baker: &Rc<ScriptedBaker>) -> Option<(HeadlessWindow, Handles, FrameSink)> {
    let handles = Rc::new(Cell::new(None));
    let sink = FrameSink::new();
    let (mounted_baker, mounted_handles, shown) = (Rc::clone(baker), Rc::clone(&handles), sink.clone());
    let options = HeadlessOptions {
        drawing: Drawing::Device,
        ..HeadlessOptions::new(WindowSize { width: 1000.0, height: 620.0 })
    };
    let opened = HeadlessWindow::open(options, move || {
        let (baker, handles, sink) = (Rc::clone(&mounted_baker), Rc::clone(&mounted_handles), shown.clone());
        view! { Gallery(baker = baker, handles = handles, sink = sink) }
    });
    let window = match opened {
        Ok(window) => window,
        Err(HeadlessError::NoWindow | HeadlessError::App { source: zgui::runtime::AppError::GpuUnavailable(_) }) => {
            eprintln!("skipped: no graphics adapter to draw on");
            return None;
        }
        Err(err) => panic!("the gallery opens: {err}"),
    };
    let handles = handles.get().expect("the gallery was built");

    Some((window, handles, sink))
}

fn shot_path(name: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../target/zgui-shots").join(format!("{name}.png"))
}

#[test]
#[ignore = "draws on the graphics device; run by hand to review the frame view and the hover preview"]
fn the_frame_view_and_hover_preview_render() {
    let baker = Rc::new(ScriptedBaker::default());
    baker.walked.borrow_mut().push(PathBuf::from("a.wowsreplay"));
    let Some((mut window, handles, sink)) = open_gallery(&baker) else { return };
    sink.push(test_card());
    handles.enter("a.wowsreplay", handles.first);
    window.advance(ms(16));
    baker.send_map(0, sea(0));
    for step in 1..=12 {
        assert!(baker.send_frame(0, sea(step)));
    }
    baker.finish(0, Ok(()));
    window.advance(FRAME_INTERVAL * 6);

    let path = shot_path("hover-preview-dark");
    window.save_png(&path).expect("the gallery is drawn to a PNG");
    println!("{}", path.display());
    drop(window);

    let baker = Rc::new(ScriptedBaker::default());
    let Some((mut window, handles, sink)) = open_gallery(&baker) else { return };
    sink.push(test_card());
    handles.enter("a.wowsreplay", handles.first);
    window.advance(DWELL + ms(100));

    let path = shot_path("hover-preview-loading-dark");
    window.save_png(&path).expect("the gallery is drawn to a PNG");
    println!("{}", path.display());
}

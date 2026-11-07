//! CPU-rendered frames shown two ways: a GTK native component fed RGBA
//! textures, and an nsh `image()` fed an encoded frame each time (a `data:`
//! URI or a file path).

mod encode;
#[cfg(target_os = "linux")]
pub mod native;
mod scene;
pub mod tab;

use std::collections::VecDeque;
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::LazyLock;
use std::sync::Mutex;
use std::sync::atomic::AtomicBool;
use std::sync::atomic::AtomicU64;
use std::sync::atomic::Ordering;
use std::time::Duration;
use std::time::Instant;

use tokio::sync::watch;

pub use self::encode::UriFormat;
pub use self::scene::FRAME_SIDE;
pub use self::scene::PanOffset;
use self::scene::Scene;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FrameMode {
    /// GTK native component: `gtk::Picture` + `gdk::MemoryTexture`.
    Native,
    /// nsh `image()` whose source is replaced each frame.
    Image(ImageTransport, UriBuffering),
}

/// What the `image()` source carries.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ImageTransport {
    /// A `data:` URI with the encoded bytes.
    Data(UriFormat),
    /// The absolute path of a BMP written to a RAM-backed directory.
    BmpFile,
}

/// How the `image()` path swaps frames.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UriBuffering {
    /// One `image()` whose source changes each frame.
    Single,
    /// Two stacked `image()`s; the back one takes the new source and is
    /// shown once it reports loaded.
    Swap,
}

impl FrameMode {
    pub const ALL: [FrameMode; 7] = [
        FrameMode::Native,
        FrameMode::Image(ImageTransport::Data(UriFormat::Bmp), UriBuffering::Single),
        FrameMode::Image(ImageTransport::Data(UriFormat::Png), UriBuffering::Single),
        FrameMode::Image(ImageTransport::Data(UriFormat::Bmp), UriBuffering::Swap),
        FrameMode::Image(ImageTransport::Data(UriFormat::Png), UriBuffering::Swap),
        FrameMode::Image(ImageTransport::BmpFile, UriBuffering::Single),
        FrameMode::Image(ImageTransport::BmpFile, UriBuffering::Swap),
    ];

    pub fn label(self) -> &'static str {
        match self {
            FrameMode::Native => "native",
            FrameMode::Image(ImageTransport::Data(UriFormat::Bmp), UriBuffering::Single) => "bmp",
            FrameMode::Image(ImageTransport::Data(UriFormat::Png), UriBuffering::Single) => "png",
            FrameMode::Image(ImageTransport::Data(UriFormat::Bmp), UriBuffering::Swap) => "bmp-swap",
            FrameMode::Image(ImageTransport::Data(UriFormat::Png), UriBuffering::Swap) => "png-swap",
            FrameMode::Image(ImageTransport::BmpFile, UriBuffering::Single) => "file",
            FrameMode::Image(ImageTransport::BmpFile, UriBuffering::Swap) => "file-swap",
        }
    }

    pub fn parse(label: &str) -> Option<FrameMode> {
        FrameMode::ALL.into_iter().find(|mode| mode.label() == label)
    }
}

/// Monotonic producer frame number.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct FrameSeq(pub u64);

pub struct RgbaFrame {
    pub seq: FrameSeq,
    pub pixels: Vec<u8>,
}

/// An `image()` source for one frame; equality is by sequence so signals skip
/// the 3 MB compare.
#[derive(Debug, Clone)]
pub struct UriFrame {
    pub seq: FrameSeq,
    pub uri: Arc<str>,
}

impl PartialEq for UriFrame {
    fn eq(&self, other: &Self) -> bool {
        self.seq == other.seq
    }
}

/// Counters reset by each once-per-second report.
#[derive(Default)]
pub struct FrameStats {
    produced: AtomicU64,
    raster_ns: AtomicU64,
    encode_ns: AtomicU64,
    encoded_bytes: AtomicU64,
    overwritten: AtomicU64,
    displayed: AtomicU64,
    display_ns: AtomicU64,
    display_errors: AtomicU64,
    loads: AtomicU64,
    load_ns: AtomicU64,
    nsh_responder_moves: AtomicU64,
    nsh_mouse_moves: AtomicU64,
    gtk_drag_updates: AtomicU64,
    gtk_scrolls: AtomicU64,
    gtk_zooms: AtomicU64,
}

impl FrameStats {
    /// A frame reached the screen; `main_thread` is the GTK-side cost when known.
    pub fn record_display(&self, main_thread: Option<Duration>) {
        self.displayed.fetch_add(1, Ordering::Relaxed);
        if let Some(cost) = main_thread {
            self.display_ns.fetch_add(cost.as_nanos() as u64, Ordering::Relaxed);
        }
    }

    pub fn record_display_error(&self) {
        self.display_errors.fetch_add(1, Ordering::Relaxed);
    }

    /// Time from an `image()`'s load start to its load event, on the engine thread.
    pub fn record_load(&self, latency: Duration) {
        self.loads.fetch_add(1, Ordering::Relaxed);
        self.load_ns.fetch_add(latency.as_nanos() as u64, Ordering::Relaxed);
    }

    /// Frames the consumer never saw because a newer one replaced them.
    pub fn record_skipped(&self, frames: u64) {
        self.overwritten.fetch_add(frames, Ordering::Relaxed);
    }

    pub fn record_input(&self, input: InputKind) {
        let counter = match input {
            InputKind::NshResponderMove => &self.nsh_responder_moves,
            InputKind::NshMouseMove => &self.nsh_mouse_moves,
            InputKind::GtkDragUpdate => &self.gtk_drag_updates,
            InputKind::GtkScroll => &self.gtk_scrolls,
            InputKind::GtkZoom => &self.gtk_zooms,
        };
        counter.fetch_add(1, Ordering::Relaxed);
    }

    fn take(counter: &AtomicU64) -> u64 {
        counter.swap(0, Ordering::Relaxed)
    }

    fn mean_ms(total_ns: &AtomicU64, count: u64) -> f64 {
        let total = FrameStats::take(total_ns);
        match count {
            0 => 0.0,
            n => total as f64 / n as f64 / 1e6,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InputKind {
    NshResponderMove,
    NshMouseMove,
    GtkDragUpdate,
    GtkScroll,
    GtkZoom,
}

/// BMP files of the file route, oldest first, deleted once a newer frame shows.
struct FrameFiles {
    dir: PathBuf,
    written: VecDeque<(FrameSeq, PathBuf)>,
}

impl FrameFiles {
    fn new() -> Self {
        // /dev/shm keeps the files in RAM; XDG_RUNTIME_DIR is tmpfs on systemd hosts.
        let base = match std::path::Path::new("/dev/shm").is_dir() {
            true => PathBuf::from("/dev/shm"),
            false => std::env::var_os("XDG_RUNTIME_DIR").map(PathBuf::from).unwrap_or_else(std::env::temp_dir),
        };
        let dir = base.join(format!("wows-toolkit-nsh-{}", std::process::id()));

        Self { dir, written: VecDeque::new() }
    }

    fn write(&mut self, seq: FrameSeq, bytes: &[u8]) -> std::io::Result<PathBuf> {
        std::fs::create_dir_all(&self.dir)?;
        let path = self.dir.join(format!("frame-{}.bmp", seq.0));
        std::fs::write(&path, bytes)?;
        self.written.push_back((seq, path.clone()));

        Ok(path)
    }

    fn release_before(&mut self, shown: FrameSeq) {
        while let Some((seq, _)) = self.written.front()
            && *seq < shown
        {
            let (_, path) = self.written.pop_front().expect("front was just checked");
            if let Err(error) = std::fs::remove_file(&path) {
                log::warn!(target: "frames", "removing {}: {error}", path.display());
            }
        }
    }
}

/// Shared between the producer thread, the GTK main thread and the engine thread.
pub struct FrameHub {
    mode: Mutex<FrameMode>,
    active: AtomicBool,
    pan: Mutex<PanOffset>,
    rgba: Mutex<Option<RgbaFrame>>,
    uri: watch::Sender<Option<UriFrame>>,
    files: Mutex<FrameFiles>,
    report: Mutex<String>,
    pub stats: FrameStats,
}

impl FrameHub {
    fn new() -> Self {
        Self {
            mode: Mutex::new(FrameMode::Native),
            active: AtomicBool::new(false),
            pan: Mutex::new(PanOffset::default()),
            rgba: Mutex::new(None),
            uri: watch::Sender::new(None),
            files: Mutex::new(FrameFiles::new()),
            report: Mutex::new("waiting for the first report".to_owned()),
            stats: FrameStats::default(),
        }
    }

    pub fn mode(&self) -> FrameMode {
        *self.mode.lock().unwrap_or_else(|e| e.into_inner())
    }

    pub fn set_mode(&self, mode: FrameMode) {
        *self.mode.lock().unwrap_or_else(|e| e.into_inner()) = mode;
        log::info!(target: "frames", "mode -> {}", mode.label());
    }

    pub fn set_active(&self, active: bool) {
        self.active.store(active, Ordering::Relaxed);
    }

    pub fn pan_by(&self, dx: f32, dy: f32) {
        let mut pan = self.pan.lock().unwrap_or_else(|e| e.into_inner());
        pan.x += dx;
        pan.y += dy;
    }

    pub fn take_rgba(&self) -> Option<RgbaFrame> {
        self.rgba.lock().unwrap_or_else(|e| e.into_inner()).take()
    }

    /// The latest `image()` source; the engine thread awaits or polls it.
    pub fn subscribe(&self) -> watch::Receiver<Option<UriFrame>> {
        self.uri.subscribe()
    }

    /// `seq` reported loaded: older frame files are no longer shown.
    pub fn frame_shown(&self, seq: FrameSeq) {
        self.files.lock().unwrap_or_else(|e| e.into_inner()).release_before(seq);
    }

    pub fn report(&self) -> String {
        self.report.lock().unwrap_or_else(|e| e.into_inner()).clone()
    }

    fn publish(&self, seq: FrameSeq, mode: FrameMode, rgba: &[u8]) {
        let transport = match mode {
            FrameMode::Native => {
                let frame = RgbaFrame { seq, pixels: rgba.to_vec() };
                if self.rgba.lock().unwrap_or_else(|e| e.into_inner()).replace(frame).is_some() {
                    self.stats.record_skipped(1);
                }
                return;
            }
            FrameMode::Image(transport, _) => transport,
        };

        let start = Instant::now();
        let uri = match transport {
            ImageTransport::Data(format) => encode::data_uri(format, rgba, FRAME_SIDE, FRAME_SIDE),
            ImageTransport::BmpFile => {
                let bytes = encode::bmp(rgba, FRAME_SIDE, FRAME_SIDE);
                let written = self.files.lock().unwrap_or_else(|e| e.into_inner()).write(seq, &bytes);
                match written {
                    Ok(path) => path.to_string_lossy().into_owned(),
                    Err(error) => {
                        log::error!(target: "frames", "writing frame {}: {error}", seq.0);
                        self.stats.record_display_error();
                        return;
                    }
                }
            }
        };
        self.stats.encode_ns.fetch_add(start.elapsed().as_nanos() as u64, Ordering::Relaxed);
        self.stats.encoded_bytes.fetch_add(uri.len() as u64, Ordering::Relaxed);

        self.uri.send_replace(Some(UriFrame { seq, uri: Arc::from(uri) }));
    }

    fn write_report(&self, elapsed: Duration) {
        let stats = &self.stats;
        let produced = FrameStats::take(&stats.produced);
        let raster_ms = FrameStats::mean_ms(&stats.raster_ns, produced);
        let encode_ms = FrameStats::mean_ms(&stats.encode_ns, produced);
        let source_kib = match produced {
            0 => 0,
            n => FrameStats::take(&stats.encoded_bytes) / n / 1024,
        };
        let displayed = FrameStats::take(&stats.displayed);
        let display_ms = FrameStats::mean_ms(&stats.display_ns, displayed);
        let loads = FrameStats::take(&stats.loads);
        let load_ms = FrameStats::mean_ms(&stats.load_ns, loads);
        let seconds = elapsed.as_secs_f64();

        let report = format!(
            "mode={} produced_fps={:.1} displayed_fps={:.1} raster_ms={raster_ms:.2} encode_ms={encode_ms:.2} \
             source_kib={source_kib} load_ms={load_ms:.2} main_upload_ms={display_ms:.3} skipped={} \
             display_errors={} input[nsh_responder_move={} nsh_mouse_move={} gtk_drag={} gtk_scroll={} gtk_zoom={}]",
            self.mode().label(),
            produced as f64 / seconds,
            displayed as f64 / seconds,
            FrameStats::take(&stats.overwritten),
            FrameStats::take(&stats.display_errors),
            FrameStats::take(&stats.nsh_responder_moves),
            FrameStats::take(&stats.nsh_mouse_moves),
            FrameStats::take(&stats.gtk_drag_updates),
            FrameStats::take(&stats.gtk_scrolls),
            FrameStats::take(&stats.gtk_zooms),
        );
        log::info!(target: "frames", "{report}");
        *self.report.lock().unwrap_or_else(|e| e.into_inner()) = report;
    }
}

/// The process-wide hub; the first access starts the producer thread.
static HUB: LazyLock<Arc<FrameHub>> = LazyLock::new(|| {
    let hub = Arc::new(FrameHub::new());
    let producer = Arc::clone(&hub);
    std::thread::Builder::new()
        .name("frame-producer".to_owned())
        .spawn(move || produce(&producer))
        .expect("spawning the frame producer thread");
    hub
});

pub fn hub() -> &'static Arc<FrameHub> {
    &HUB
}

fn produce(hub: &FrameHub) {
    const PERIOD: Duration = Duration::from_nanos(1_000_000_000 / 30);

    let mut scene = Scene::new();
    let mut seq = 0u64;
    let mut next = Instant::now();
    let mut report_start = Instant::now();

    loop {
        if !hub.active.load(Ordering::Relaxed) {
            std::thread::sleep(Duration::from_millis(50));
            next = Instant::now();
            report_start = Instant::now();
            continue;
        }

        let mode = hub.mode();
        let pan = *hub.pan.lock().unwrap_or_else(|e| e.into_inner());

        let start = Instant::now();
        let rgba = scene.render(pan);
        hub.stats.raster_ns.fetch_add(start.elapsed().as_nanos() as u64, Ordering::Relaxed);

        seq += 1;
        hub.publish(FrameSeq(seq), mode, rgba);
        hub.stats.produced.fetch_add(1, Ordering::Relaxed);

        let since_report = report_start.elapsed();
        if since_report >= Duration::from_secs(1) {
            hub.write_report(since_report);
            report_start = Instant::now();
        }

        next += PERIOD;
        let now = Instant::now();
        match next.checked_duration_since(now) {
            Some(wait) => std::thread::sleep(wait),
            // Behind schedule: drop the backlog rather than burst.
            None => next = now,
        }
    }
}

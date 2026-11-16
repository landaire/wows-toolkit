//! TEMPORARY: traces every job on the background pool. Not for commit.

use std::backtrace::Backtrace;
use std::io::Write;
use std::pin::Pin;
use std::sync::Arc;
use std::sync::Mutex;
use std::sync::atomic::AtomicUsize;
use std::sync::atomic::Ordering;
use std::sync::mpsc;
use std::time::Instant;

use futures::executor::ThreadPool;
use zgui::reactive::BackgroundSpawner;

struct Event {
    id: usize,
    test: String,
    trace: Backtrace,
    queued_at: Instant,
    started: Instant,
    ended: Instant,
}

struct Traced {
    pool: ThreadPool,
    events: Mutex<mpsc::Sender<Event>>,
    next: AtomicUsize,
}

impl BackgroundSpawner for Traced {
    fn spawn(&self, future: Pin<Box<dyn Future<Output = ()> + Send>>) {
        self.pool.spawn_ok(future);
    }

    fn spawn_blocking(&self, work: Box<dyn FnOnce() + Send>) {
        let id = self.next.fetch_add(1, Ordering::Relaxed);
        let test = std::thread::current().name().unwrap_or("?").to_owned();
        let trace = Backtrace::force_capture();
        let queued_at = Instant::now();
        let events = self.events.lock().unwrap().clone();
        self.pool.spawn_ok(async move {
            let started = Instant::now();
            work();
            let ended = Instant::now();
            let _ = events.send(Event { id, test, trace, queued_at, started, ended });
        });
    }
}

fn caller(trace: &Backtrace) -> String {
    let text = format!("{trace}");
    let mut lines = text.lines().filter(|line| line.contains(": wows_toolkit_zgui::") && !line.contains("pool_trace"));
    lines.next().map(|line| line.trim().to_owned()).unwrap_or_else(|| "?".to_owned())
}

extern "C" fn install() {
    let epoch = Instant::now();
    let (tx, rx) = mpsc::channel::<Event>();
    std::thread::spawn(move || {
        let mut file = std::fs::File::create("/var/mnt/build/wt-bisect/pool-trace.log").unwrap();
        for event in rx {
            let name = caller(&event.trace);
            let _ = writeln!(
                file,
                "job={} queued={:.3} wait_ms={} run_ms={} ended={:.3} test={} at={}",
                event.id,
                (event.queued_at - epoch).as_secs_f64(),
                (event.started - event.queued_at).as_millis(),
                (event.ended - event.started).as_millis(),
                (event.ended - epoch).as_secs_f64(),
                event.test,
                name,
            );
            let _ = file.flush();
        }
    });
    let pool = ThreadPool::builder().pool_size(4).name_prefix("zgui-worker-").create().unwrap();
    let traced = Traced { pool, events: Mutex::new(tx), next: AtomicUsize::new(0) };
    zgui::reactive::set_background_spawner(Arc::new(traced)).unwrap();
}

#[used]
#[unsafe(link_section = ".init_array")]
static INSTALL: extern "C" fn() = install;

//! Where this session's log goes, and what a crash leaves behind for the next
//! one to report.
//!
//! The egui app keeps an hourly-rotated log beside the executable, writes a
//! panic log into the shared storage directory, and shows what the last run left
//! there on the next launch (`app.rs`'s `init_logging`, `panic_log_path` and the
//! Crash Detected window). Both apps read the same two places, so a report from
//! either names the same files.

use std::io::Write as _;
use std::path::Path;
use std::path::PathBuf;

/// The crates whose events are kept, at the levels the egui app keeps them.
fn targets() -> tracing_subscriber::filter::Targets {
    tracing_subscriber::filter::Targets::new()
        .with_target("wows_toolkit_gpui", tracing::Level::DEBUG)
        .with_target("wows_replay_insights", tracing::Level::DEBUG)
        .with_target("wows_replays", tracing::Level::INFO)
        .with_target(wows_data_mgr::LOG_TARGET, tracing::Level::INFO)
}

/// Where the rotated log files live: beside the executable, as the egui app
/// writes them, so one Copy Latest Log finds either app's.
pub fn log_dir() -> PathBuf {
    std::env::current_exe()
        .ok()
        .and_then(|exe| exe.parent().map(Path::to_path_buf))
        .unwrap_or_else(|| PathBuf::from("."))
}

/// Starts logging: to the rotated file when `to_file`, and to stderr either way.
///
/// The returned guard flushes the file on drop and has to be held for the life of
/// the process; without it the last events written are lost. `RUST_LOG` still
/// overrides what reaches stderr, which is how a session is asked for more.
pub fn init(to_file: bool) -> Option<tracing_appender::non_blocking::WorkerGuard> {
    use tracing_subscriber::Layer as _;
    use tracing_subscriber::layer::SubscriberExt as _;

    let env_filter = tracing_subscriber::EnvFilter::try_from_default_env()
        .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info"));
    let console = tracing_subscriber::fmt::Layer::new().with_ansi(true).with_target(true).with_filter(env_filter);

    let file = to_file
        .then(|| {
            tracing_appender::rolling::Builder::new()
                .rotation(tracing_appender::rolling::Rotation::HOURLY)
                .max_log_files(3)
                .filename_prefix(LOG_PREFIX)
                .build(log_dir())
                .inspect_err(|err| eprintln!("no log file: {err}"))
                .ok()
        })
        .flatten();

    let (writer, guard) = match file {
        Some(file) => {
            let (writer, guard) = tracing_appender::non_blocking(file);
            (Some(writer), Some(guard))
        }
        None => (None, None),
    };

    let subscriber = tracing_subscriber::registry().with(console).with(writer.map(|writer| {
        tracing_subscriber::fmt::Layer::new()
            .with_writer(writer)
            .with_timer(tracing_subscriber::fmt::time::LocalTime::rfc_3339())
            .with_ansi(false)
            .with_target(true)
            .with_filter(targets())
    }));
    let _ = tracing::subscriber::set_global_default(subscriber);

    guard
}

/// The name every rotated log file starts with. Shared with the egui app, whose
/// files this one sits beside.
const LOG_PREFIX: &str = "wows_toolkit.log";

/// The newest rotated log file, by name.
///
/// `tracing_appender` suffixes the prefix with a sortable `YYYY-MM-DD-HH`, so the
/// highest name is the newest file. Read that way rather than by modification
/// time, which a copy or a sync tool rewrites.
pub fn newest_log() -> Option<PathBuf> {
    newest_log_in(&log_dir())
}

fn newest_log_in(dir: &Path) -> Option<PathBuf> {
    let mut newest: Option<PathBuf> = None;
    for entry in std::fs::read_dir(dir).ok()?.flatten() {
        let path = entry.path();
        if !path.file_name().is_some_and(|name| name.to_string_lossy().starts_with(LOG_PREFIX)) {
            continue;
        }
        if newest.as_ref().is_none_or(|held| held.file_name() < path.file_name()) {
            newest = Some(path);
        }
    }
    newest
}

/// The file a panic is written to, which the next launch reports.
///
/// In the shared storage directory, which is where the egui app writes it, so
/// either app reports a crash in the other.
pub fn panic_log_path() -> PathBuf {
    let name = PathBuf::from("wows_toolkit_panic.log");
    match wows_toolkit_config::storage_dir() {
        Some(dir) => dir.join(name),
        None => name,
    }
}

/// Writes a panic on the main thread to the panic log, then lets the default
/// hook run.
///
/// Only the main thread: a background task that panics is caught where it is
/// spawned (the replay parse does this) and does not end the session, so
/// reporting it as a crash on the next launch would be wrong.
pub fn install_panic_hook() {
    let main_thread = std::thread::current().id();
    let default_hook = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        if std::thread::current().id() != main_thread {
            default_hook(info);
            return;
        }

        if let Ok(mut file) = std::fs::File::create(panic_log_path()) {
            let _ = writeln!(file, "{info}");
            let _ = writeln!(file, "Backtrace:\n{}", std::backtrace::Backtrace::force_capture());
        }
        default_hook(info);
    }));
}

/// What the last run's crash left, removing it as it is read.
///
/// Taken rather than read: the report is shown once, and a file left behind would
/// be reported at every launch until the reader found it themselves.
pub fn take_crash_report() -> Option<String> {
    let path = panic_log_path();
    let report = std::fs::read_to_string(&path).ok()?;
    if let Err(err) = std::fs::remove_file(&path) {
        tracing::warn!(path = %path.display(), error = %err, "the crash report could not be cleared");
    }
    (!report.trim().is_empty()).then_some(report)
}

#[cfg(test)]
mod tests {
    /// The newest file is the one with the highest name, which is what the hourly
    /// suffix makes it, rather than the one most recently touched.
    #[test]
    fn the_newest_log_is_the_highest_name() {
        let dir = tempfile::tempdir().expect("a temp directory");
        for name in ["wows_toolkit.log.2026-09-27-09", "wows_toolkit.log.2026-09-28-11", "notes.txt"] {
            std::fs::write(dir.path().join(name), "x").expect("the file is written");
        }

        // Touched last, and still not the newest.
        std::fs::write(dir.path().join("wows_toolkit.log.2026-09-27-09"), "x").expect("the file is rewritten");

        let newest = super::newest_log_in(dir.path()).expect("a log file is found");
        assert_eq!(newest.file_name().unwrap(), "wows_toolkit.log.2026-09-28-11");
    }
}

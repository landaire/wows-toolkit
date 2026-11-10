//! Where this session's log goes.
//!
//! The rotated file sits beside the executable under the name the other front
//! ends use (`wows_toolkit_viewmodel::logging`), so one Copy Latest Log finds
//! any of them, and a panic on the main thread is written where the next
//! launch reports it.

use tracing_subscriber::Layer as _;
use tracing_subscriber::layer::SubscriberExt as _;
use tracing_subscriber::util::SubscriberInitExt as _;
use wows_toolkit_viewmodel::logging::LOG_PREFIX;
use wows_toolkit_viewmodel::logging::log_dir;

/// The crates whose events the file keeps, at the levels the other front ends
/// keep them.
fn targets() -> tracing_subscriber::filter::Targets {
    tracing_subscriber::filter::Targets::new()
        .with_target("wows_toolkit_nsh", tracing::Level::DEBUG)
        .with_target("wows_toolkit_viewmodel", tracing::Level::DEBUG)
        .with_target("wows_replay_insights", tracing::Level::DEBUG)
        .with_target("wows_replays", tracing::Level::INFO)
}

/// Starts logging: to the rotated file when `to_file`, and to stderr either
/// way, `RUST_LOG` governing stderr. Events of the `log` crate (nsh's own) are
/// carried over. The returned guard flushes the file when dropped and is held
/// for the life of the process.
pub fn init(to_file: bool) -> Option<tracing_appender::non_blocking::WorkerGuard> {
    // An unset or unparsable RUST_LOG shows info and above, the usual console level.
    let env_filter = tracing_subscriber::EnvFilter::try_from_default_env()
        .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info"));
    let console = tracing_subscriber::fmt::Layer::new().with_target(true).with_filter(env_filter);

    let file = if to_file {
        tracing_appender::rolling::Builder::new()
            .rotation(tracing_appender::rolling::Rotation::HOURLY)
            .max_log_files(3)
            .filename_prefix(LOG_PREFIX)
            .build(log_dir())
            .inspect_err(|err| eprintln!("no log file: {err}"))
            .ok()
    } else {
        None
    };

    let (writer, guard) = match file {
        Some(file) => {
            let (writer, guard) = tracing_appender::non_blocking(file);
            (Some(writer), Some(guard))
        }
        None => (None, None),
    };

    let file_layer = writer.map(|writer| {
        tracing_subscriber::fmt::Layer::new()
            .with_writer(writer)
            .with_timer(tracing_subscriber::fmt::time::LocalTime::rfc_3339())
            .with_ansi(false)
            .with_target(true)
            .with_filter(targets())
    });
    if let Err(err) = tracing_subscriber::registry().with(console).with(file_layer).try_init() {
        eprintln!("logging was already set up: {err}");
    }

    guard
}

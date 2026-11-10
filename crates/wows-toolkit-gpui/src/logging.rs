//! This app's tracing subscriber. Where the log goes and what a crash leaves
//! behind are `wows_toolkit_viewmodel::logging`, shared with the egui app.

use wows_toolkit_viewmodel::logging::LOG_PREFIX;
use wows_toolkit_viewmodel::logging::log_dir;

/// The crates whose events are kept, at the levels the egui app keeps them.
fn targets() -> tracing_subscriber::filter::Targets {
    tracing_subscriber::filter::Targets::new()
        .with_target("wows_toolkit_gpui", tracing::Level::DEBUG)
        .with_target("wows_replay_insights", tracing::Level::DEBUG)
        .with_target("wows_replays", tracing::Level::INFO)
        .with_target(wows_data_mgr::LOG_TARGET, tracing::Level::INFO)
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

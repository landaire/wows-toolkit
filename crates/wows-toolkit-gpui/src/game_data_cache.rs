//! The game-data cache operations the Settings tab starts, on GPUI's
//! executors. The operations themselves and the state they report into are
//! `wows_toolkit_replay_data::cache`.
//!
//! Every operation here reaches the network or walks a directory of several
//! gigabytes, so all of them run off the UI thread and report back by channel.

use std::path::PathBuf;

use gpui_kit::App;
use gpui_kit::AppContext as _;
use gpui_kit::AsyncApp;
use gpui_kit::Context;
use gpui_kit::Entity;
use wows_data_mgr::download_repo::BuildUpdateStatus;
use wows_data_mgr::dump::CacheStats;
use wows_toolkit_replay_data::cache;
use wows_toolkit_replay_data::cache::CacheJob;
use wows_toolkit_replay_data::cache::CacheOutcome;
use wows_toolkit_replay_data::cache::CacheState;
use wows_toolkit_replay_data::cache::MeasureGeneration;
use wows_toolkit_replay_data::cache::Progress;
use wows_toolkit_replay_data::error::CacheError;

use crate::http;
use crate::runtime;

/// Measures the cache off the UI thread and hands the result to `apply`.
///
/// `generation` comes back with the answer so the caller can drop one taken
/// against a directory that has since been left.
pub fn measure<V: 'static>(
    base: PathBuf,
    generation: MeasureGeneration,
    cx: &mut Context<V>,
    apply: impl FnOnce(&mut V, CacheStats, MeasureGeneration, &mut Context<V>) + 'static,
) {
    cx.spawn(async move |view, cx: &mut AsyncApp| {
        let measured = cx.background_spawn(async move { cache::measure(&base) }).await;
        let _ = view.update(cx, |view, cx| apply(view, measured, generation, cx));
    })
    .detach();
}

/// Runs an operation against the cache and reports back through `apply`.
///
/// `proxy_url` is the setting as stored: empty means direct.
fn run<V, Work, Fut>(
    state: impl Fn(&mut V) -> &mut CacheState + Copy + 'static,
    job: CacheJob,
    proxy_url: String,
    view: &Entity<V>,
    cx: &mut App,
    work: Work,
    apply: impl FnOnce(&mut V, CacheOutcome, &mut Context<V>) + 'static,
) where
    V: 'static,
    Work: FnOnce(reqwest::Client, ProgressSink) -> Fut + Send + 'static,
    Fut: Future<Output = Result<CacheOutcome, CacheError>>,
{
    let view = view.clone();
    // Callers can hold the owner entity while requesting an operation.
    cx.defer(move |cx| {
        view.update(cx, |view, cx| {
            state(view).start(job);
            cx.notify();
        });

        let (progress_tx, mut progress_rx) = futures::channel::mpsc::unbounded::<Progress>();
        let view = view.clone();

        // The bar follows the work rather than waiting for it, so a long
        // validation is visibly running rather than looking hung.
        let watched = view.clone();
        cx.spawn(async move |cx: &mut AsyncApp| {
            use futures::StreamExt as _;
            while let Some(step) = progress_rx.next().await {
                watched.update(cx, |view, cx| {
                    state(view).progress = Some(step);
                    cx.notify();
                });
            }
        })
        .detach();

        cx.spawn(async move |cx: &mut AsyncApp| {
            let outcome = match http::client(&proxy_url, reqwest::redirect::Policy::default()) {
                Ok(client) => {
                    let sink = ProgressSink(progress_tx);
                    runtime::block_on(cx, move || work(client, sink))
                        .await
                        .unwrap_or_else(|err| CacheOutcome::Failed(err.to_string()))
                }
                Err(err) => CacheOutcome::Failed(err.to_string()),
            };

            view.update(cx, |view, cx| {
                state(view).finish();
                if let CacheOutcome::Failed(reason) = &outcome {
                    state(view).failure = Some(reason.clone());
                }
                apply(view, outcome, cx);
                cx.notify();
            });
        })
        .detach();
    });
}

/// Forwards progress from the `wows_data_mgr` calls to the UI.
///
/// Unbounded and non-blocking: a progress report must never hold up the work
/// reporting it, and a report that arrives after the view is gone is dropped.
pub struct ProgressSink(futures::channel::mpsc::UnboundedSender<Progress>);

impl ProgressSink {
    fn report(&self, step: Progress) {
        let _ = self.0.unbounded_send(step);
    }
}

/// Brings an older cache layout up to date and drops what nothing references,
/// off the UI thread (see `wows_toolkit_replay_data::cache::maintain`).
pub fn maintain(base: PathBuf, cx: &App) {
    cx.background_spawn(async move { cache::maintain(&base) }).detach();
}

/// Asks the repository which cached builds have newer data.
pub fn check_for_updates<V: 'static>(
    state: impl Fn(&mut V) -> &mut CacheState + Copy + 'static,
    base: PathBuf,
    known_tip: Option<String>,
    proxy_url: String,
    view: &Entity<V>,
    cx: &mut App,
    apply: impl FnOnce(&mut V, CacheOutcome, &mut Context<V>) + 'static,
) {
    run(
        state,
        CacheJob::Checking,
        proxy_url,
        view,
        cx,
        move |client, _progress| async move { cache::check_for_updates(&client, &base, known_tip.as_deref()).await },
        apply,
    );
}

/// Checks every cached object against the repository and its own hash.
pub fn validate<V: 'static>(
    state: impl Fn(&mut V) -> &mut CacheState + Copy + 'static,
    base: PathBuf,
    proxy_url: String,
    view: &Entity<V>,
    cx: &mut App,
    apply: impl FnOnce(&mut V, CacheOutcome, &mut Context<V>) + 'static,
) {
    run(
        state,
        CacheJob::Validating,
        proxy_url,
        view,
        cx,
        move |client, progress| async move { cache::validate(&client, &base, |step| progress.report(step)).await },
        apply,
    );
}

/// Asks the repository what it has for each of `builds`, and what fetching them
/// all would come to.
pub fn plan<V: 'static>(
    state: impl Fn(&mut V) -> &mut CacheState + Copy + 'static,
    base: PathBuf,
    builds: Vec<(u32, Option<String>)>,
    proxy_url: String,
    view: &Entity<V>,
    cx: &mut App,
    apply: impl FnOnce(&mut V, CacheOutcome, &mut Context<V>) + 'static,
) {
    run(
        state,
        CacheJob::Planning,
        proxy_url,
        view,
        cx,
        move |client, _progress| async move { cache::plan(&client, &base, &builds).await },
        apply,
    );
}

/// Re-fetches the named builds, whether they are stale or damaged.
pub fn download<V: 'static>(
    state: impl Fn(&mut V) -> &mut CacheState + Copy + 'static,
    base: PathBuf,
    builds: Vec<BuildUpdateStatus>,
    proxy_url: String,
    view: &Entity<V>,
    cx: &mut App,
    apply: impl FnOnce(&mut V, CacheOutcome, &mut Context<V>) + 'static,
) {
    run(
        state,
        CacheJob::Downloading,
        proxy_url,
        view,
        cx,
        move |client, progress| async move { cache::download(&client, &base, &builds, |step| progress.report(step)).await },
        apply,
    );
}

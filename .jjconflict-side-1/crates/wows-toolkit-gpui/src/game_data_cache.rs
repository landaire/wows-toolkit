//! Schedules `wows_toolkit_services::game_data_cache` jobs for the Settings
//! drawer, off the UI thread, and folds what they report into the view's
//! `CacheState`.

use std::path::PathBuf;

use gpui_kit::App;
use gpui_kit::AppContext as _;
use gpui_kit::AsyncApp;
use gpui_kit::Context;
use gpui_kit::Entity;
use wows_data_mgr::download_repo::BuildUpdateStatus;
use wows_data_mgr::dump::CacheStats;
use wows_toolkit_services::game_data_cache::CacheJob;
use wows_toolkit_services::game_data_cache::CacheOutcome;
use wows_toolkit_services::game_data_cache::CacheState;
use wows_toolkit_services::game_data_cache::Progress;

use crate::runtime;

/// Measures the cache off the UI thread and hands the result to `apply`.
///
/// `generation` comes back with the answer so the caller can drop one taken
/// against a directory that has since been left.
pub fn measure<V: 'static>(
    base: PathBuf,
    generation: u64,
    cx: &mut Context<V>,
    apply: impl FnOnce(&mut V, CacheStats, u64, &mut Context<V>) + 'static,
) {
    cx.spawn(async move |view, cx: &mut AsyncApp| {
        let measured = cx.background_spawn(async move { wows_toolkit_services::game_data_cache::measure(&base) }).await;
        let _ = view.update(cx, |view, cx| apply(view, measured, generation, cx));
    })
    .detach();
}

/// Runs a job against the cache and reports back through `apply`.
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
    Fut: Future<Output = CacheOutcome>,
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
            let outcome = match wows_toolkit_services::http::client(
                crate::http::USER_AGENT,
                &proxy_url,
                reqwest::redirect::Policy::default(),
            ) {
                Ok(client) => {
                    let sink = ProgressSink(progress_tx);
                    runtime::block_on(cx, move || work(client, sink)).await
                }
                Err(err) => CacheOutcome::Failed(err.to_string()),
            };

            view.update(cx, |view, cx| {
                state(view).finish(&outcome);
                apply(view, outcome, cx);
                cx.notify();
            });
        })
        .detach();
    });
}

/// Forwards a job's progress to the UI.
///
/// Unbounded and non-blocking: a progress report must never hold up the work
/// reporting it, and a report that arrives after the view is gone is dropped.
struct ProgressSink(futures::channel::mpsc::UnboundedSender<Progress>);

impl ProgressSink {
    fn report(&self, progress: Progress) {
        let _ = self.0.unbounded_send(progress);
    }
}

/// Runs `game_data_cache::maintain` off the UI thread.
pub fn maintain(base: PathBuf, cx: &App) {
    cx.background_spawn(async move { wows_toolkit_services::game_data_cache::maintain(&base) }).detach();
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
        move |client, _progress| async move {
            wows_toolkit_services::game_data_cache::check_for_updates(&client, &base, known_tip.as_deref()).await
        },
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
        move |client, progress| async move {
            wows_toolkit_services::game_data_cache::validate(&client, &base, |step| progress.report(step)).await
        },
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
        move |client, _progress| async move { wows_toolkit_services::game_data_cache::plan(&client, &base, &builds).await },
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
        move |client, progress| async move {
            wows_toolkit_services::game_data_cache::download(&client, &base, &builds, |step| progress.report(step))
                .await
        },
        apply,
    );
}

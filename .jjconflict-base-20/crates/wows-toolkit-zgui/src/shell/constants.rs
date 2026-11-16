//! Brings builds' game constants up to date on the constants worker, with the status bar and the
//! toasts saying when one is being generated and how that went.

use std::path::PathBuf;
use std::time::Duration;

use rust_i18n::t;
use wows_toolkit_services::constants::BUILDS_PER_SWEEP;
use zgui::task::blocking;

use crate::services::Job;
use crate::services::JobHandle;
use crate::services::constants::ConstantsRequest;
use crate::services::constants::ConstantsWorker;
use crate::services::constants::Fallback;
use crate::services::constants::Missed;
use crate::services::constants::Outcome;
use crate::services::constants::Provenance;
use crate::services::constants::Step;
use crate::services::constants::Wanted;
use crate::services::constants::cached_current;
use crate::services::progress;
use crate::shell::Shell;

/// Brings the constants of each of `wanted` up to date, and answers with what became of each build
/// it took up. A build already being updated is left to that update. Of the builds the cache does
/// not answer, the first [`BUILDS_PER_SWEEP`] are taken up and the rest wait for a later call, as
/// each one asks GitHub.
pub async fn update(shell: Shell, wanted: Vec<Wanted>) -> Vec<(u32, Outcome)> {
    let services = shell.services();
    let claims = services.constants.claim(wanted);
    if claims.is_empty() {
        return Vec::new();
    }

    let checked: Vec<Wanted> = claims.iter().map(|claim| claim.wanted().clone()).collect();
    let fits: Vec<bool> = blocking(move || checked.iter().map(cached_current).collect()).await;

    let mut outcomes = Vec::new();
    let mut asking = Vec::new();
    for (claim, fits) in claims.into_iter().zip(fits) {
        let build = claim.wanted().build;
        if fits {
            outcomes.push((build, Outcome::Current { provenance: Provenance::Cached, unpublished: None }));
        } else if asking.len() < BUILDS_PER_SWEEP {
            asking.push(claim);
        }
    }

    let Some(worker) = services.supervision.constants_worker() else {
        for claim in asking {
            let wanted = claim.wanted();
            let cached = wows_toolkit_services::constants::is_cached(wanted.build);
            let fallback = if cached { Fallback::Kept } else { Fallback::Embedded };
            let outcome =
                Outcome::Fallback { fallback, missed: Missed::WorkerStopped { reason: "it did not start".to_owned() } };
            say(shell, &release_label(wanted), &outcome, false);
            outcomes.push((wanted.build, outcome));
        }
        return outcomes;
    };
    let install = services
        .settings
        .read_untracked(|settings| settings.wows_dir.clone())
        .filter(|dir| !dir.is_empty())
        .map(PathBuf::from);

    for claim in asking {
        let build = claim.wanted().build;
        let outcome = follow(shell, &worker, claim.wanted().clone(), install.clone()).await;
        outcomes.push((build, outcome));
    }

    outcomes
}

/// Asks the worker for `wanted`, naming a generation on the status bar while it runs.
async fn follow(shell: Shell, worker: &ConstantsWorker, wanted: Wanted, install: Option<PathBuf>) -> Outcome {
    let services = shell.services();
    let build = wanted.build;
    let label = release_label(&wanted);
    let (steps, reported) = futures::channel::mpsc::unbounded();
    let reply = worker.update(ConstantsRequest { wanted, install, steps });

    let mut generating: Option<JobHandle> = None;
    let outcome = progress::follow(reply, reported, |step| match step {
        Step::Generating => {
            tracing::info!(build, "constants: generating from the installed client");
            if let Some(toasts) = shell.try_toasts() {
                toasts.info(t!("ui.replay.constants_generating", version = label).into_owned());
            }
            let name = t!("ui.app.status_constants_generating", version = label).into_owned();
            generating = Some(services.jobs.start(Job { name, progress: None }));
        }
    })
    .await;

    let generated = generating.take().is_some();
    say(shell, &label, &outcome, generated);

    outcome
}

/// Says how a build's update went, when there is anything to say.
fn say(shell: Shell, label: &str, outcome: &Outcome, generated: bool) {
    match outcome {
        Outcome::Current { provenance: Provenance::Generated { took }, .. } => {
            if let Some(toasts) = shell.try_toasts() {
                toasts.ok(t!("ui.replay.constants_generated", version = label, seconds = seconds(*took)).into_owned());
            }
        }
        // The caller says what was fetched, all builds at once.
        Outcome::Current { .. } => {}
        Outcome::Fallback { fallback, missed } => {
            tracing::warn!(version = label, %missed, ?fallback, "constants: none that fit could be had");
            // A listing of old replays names many builds the install has no client for. Those are
            // only logged; a generation that ran, or constants that could not be saved, are said.
            if !generated && matches!(missed, Missed::NoClient { .. }) {
                return;
            }
            if let Some(toasts) = shell.try_toasts() {
                toasts.warn(failure_said(label, missed, *fallback));
            }
        }
    }
}

/// The release a toast names a build by, or its number when no replay named its release.
fn release_label(wanted: &Wanted) -> String {
    match &wanted.release {
        Some(release) => release.clone(),
        None => t!("ui.replay.constants_build_label", build = wanted.build).into_owned(),
    }
}

/// Why a build's constants could not be updated, and what decoding uses instead.
fn failure_said(label: &str, missed: &Missed, fallback: Fallback) -> String {
    t!(
        "ui.replay.constants_generation_failed",
        version = label,
        reason = missed.to_string(),
        fallback = fallback_said(fallback)
    )
    .into_owned()
}

fn fallback_said(fallback: Fallback) -> String {
    match fallback {
        Fallback::Kept => t!("ui.replay.constants_fallback_kept"),
        Fallback::Published { build } => t!("ui.replay.constants_fallback_published", build = build),
        Fallback::Embedded => t!("ui.replay.constants_fallback_embedded"),
    }
    .into_owned()
}

fn seconds(took: Duration) -> String {
    format!("{:.1}", took.as_secs_f64())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_failure_names_its_reason_and_fallback() {
        let missed = Missed::Store { reason: "the disk is full".to_owned() };

        assert_eq!(
            failure_said("15.10.0", &missed, Fallback::Embedded),
            "Game constants for 15.10.0 could not be updated: the constants could not be saved: the disk is full. \
             Using the built-in constants."
        );
        assert_eq!(
            failure_said("15.10.0", &missed, Fallback::Published { build: 13_357_625 }),
            "Game constants for 15.10.0 could not be updated: the constants could not be saved: the disk is full. \
             Using the constants published for build 13357625."
        );
    }

    #[test]
    fn a_build_with_no_release_is_named_by_number() {
        assert_eq!(release_label(&Wanted { build: 13_533_378, release: None }), "build 13533378");
    }
}

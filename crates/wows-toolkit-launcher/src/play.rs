//! Starting the game off the UI thread: the runner and its Wine prefix, the install view, the
//! WGC relay that signs the client in as the chosen account, and the session's lifetime.

use std::path::Path;
use std::path::PathBuf;
use std::process::Command;
use std::sync::Arc;
use std::time::Duration;

use wgc_ipc::AccountEvent;
use wgc_ipc::AccountResponder;
use wgc_ipc::GameLanguage;
use wgc_ipc::RelayBinaries;
use wgc_ipc::RelayListener;
use wgc_launch::GameHandle;
use wgc_launch::GameProcess;
use wgc_launch::InstallView;
use wgc_launch::LaunchArgs;
use wgc_launch::LaunchSpec;
use wgc_launch::ResolvedRunner;
use wgc_launch::RunnerChoice;
use wgc_launch::RuntimeDirs;
use wgc_launch::WindowsProgram;

use crate::error::LaunchFailure;
use crate::game::GameEvent;
use crate::game::GameExit;
use crate::game::LaunchId;
use crate::launcher::Event;
use crate::launcher::EventSender;
use crate::settings::AccountKey;

/// Decorates the runner command before it spawns, for process hardening.
pub(crate) type PrepareCommand = Box<dyn FnOnce(&mut Command) + Send>;

/// Everything a launch needs, gathered on the UI thread.
pub(crate) struct LaunchPlan {
    pub(crate) launch: LaunchId,
    pub(crate) runner: RunnerChoice,
    pub(crate) dirs: RuntimeDirs,
    pub(crate) install: PathBuf,
    pub(crate) args: LaunchArgs,
    pub(crate) program: Program,
}

pub(crate) enum Program {
    /// The client, signed in through the relay as one account.
    SignIn(SignIn),
    /// The stub playing a replay; offline, no relay.
    Replay(PathBuf),
}

pub(crate) struct SignIn {
    pub(crate) key: AccountKey,
    pub(crate) session: Arc<wgc_auth::StoredSession>,
    pub(crate) tracking_id: wgc_core::TrackingId,
    pub(crate) credentials: wgc_auth::CredentialStore,
}

/// The runner, view and prefix work, which blocks for seconds on a first launch.
struct Prepared {
    runner: ResolvedRunner,
    view: InstallView,
    /// The relay's copy inside the prefix, for a sign-in.
    relay_exe: Option<PathBuf>,
}

/// Starts the launch and follows the game session to its end, reporting progress as events.
pub(crate) async fn run(plan: LaunchPlan, prepare: PrepareCommand, sender: EventSender) {
    let launch = plan.launch;
    let install = plan.install.clone();
    let sign_in = matches!(plan.program, Program::SignIn(_));

    let (process, serving) = match start(plan, prepare, &sender).await {
        Ok(started) => started,
        Err(failure) => {
            sender.send(Event::Game(GameEvent::Failed { launch, failure }));
            return;
        }
    };

    if sign_in {
        let sender = sender.clone();
        let handle = process.handle();
        let watching = spawn_detached("game-client-watch", move || watch_for_client(launch, &handle, &sender));
        if let Err(err) = watching {
            tracing::warn!("could not start watching for the game client: {err}");
        }
    }

    let waiting = spawn_detached("game-session", move || {
        let waited = process.wait();
        // The relay outlives the stub but not the session; nothing connects after this.
        if let Some(serving) = serving {
            serving.abort();
        }

        let status = match waited {
            Ok(exit) => Some(exit.runner_status),
            Err(err) => {
                tracing::warn!("could not wait for the game session: {err}");
                None
            }
        };
        sender.send(Event::Game(GameEvent::Exited { launch, exit: GameExit { install, status } }));
    });
    // The exit then goes unreported, so the launcher shows the game running until it restarts.
    if let Err(err) = waiting {
        tracing::error!("could not start following the game session: {err}");
    }
}

/// Runs `work` on its own thread. A game session can outlive the toolkit, and a runtime shutdown
/// waits for its blocking tasks but not for plain threads.
pub(crate) fn spawn_detached(name: &str, work: impl FnOnce() + Send + 'static) -> std::io::Result<()> {
    std::thread::Builder::new().name(name.to_owned()).spawn(work).map(drop)
}

/// Spawns the session. The relay listener, for a sign-in, serves until the returned task is
/// aborted.
async fn start(
    plan: LaunchPlan,
    prepare: PrepareCommand,
    sender: &EventSender,
) -> Result<(GameProcess, Option<tokio::task::JoinHandle<()>>), LaunchFailure> {
    let LaunchPlan { launch, runner, dirs, install, args, program } = plan;

    let signs_in = matches!(program, Program::SignIn(_));
    let Prepared { runner, view, relay_exe } =
        tokio::task::spawn_blocking(move || prepare_launch(&runner, &dirs, &install, signs_in)).await??;

    let (program, serving) = match program {
        Program::Replay(replay) => (WindowsProgram::replay(&view, &replay), None),
        Program::SignIn(sign_in) => {
            let Some(relay_exe) = relay_exe else {
                return Err(LaunchFailure::NoWinePrefix);
            };
            let language = game_language(&view, sign_in.key.realm);
            let listener = RelayListener::bind().await?;
            let env = listener.relay_env();
            let responder = account_responder(launch, sign_in, language, sender);
            let serving = tokio::spawn(async move {
                if let Err(err) = listener.serve(Arc::new(responder)).await {
                    tracing::warn!("the Game Center relay listener stopped: {err}");
                }
            });
            let program = WindowsProgram {
                exe: relay_exe,
                args: vec![wgc_launch::dos_path(&view.game_exe())],
                env,
                shared_dirs: Vec::new(),
            };
            (program, Some(serving))
        }
    };

    let spawned = tokio::task::spawn_blocking(move || {
        wgc_launch::spawn(LaunchSpec { runner: &runner, view: &view, program, args: &args }, prepare)
    })
    .await;
    let spawned = match spawned {
        Ok(Ok(process)) => Ok(process),
        Ok(Err(err)) => Err(LaunchFailure::from(err)),
        Err(err) => Err(LaunchFailure::from(err)),
    };
    match spawned {
        Ok(process) => Ok((process, serving)),
        Err(failure) => {
            if let Some(serving) = serving {
                serving.abort();
            }
            Err(failure)
        }
    }
}

fn prepare_launch(
    choice: &RunnerChoice,
    dirs: &RuntimeDirs,
    install: &Path,
    signs_in: bool,
) -> Result<Prepared, LaunchFailure> {
    let runner = wgc_launch::Runner::detect(choice, dirs)?;
    if runner.first_run_work().prefix_creation {
        runner.initialize_prefix()?;
    }

    let view = InstallView::prepare(install, dirs)?;

    let relay_exe = if signs_in {
        let drive_c = runner.prefix().drive_c().ok_or(LaunchFailure::NoWinePrefix)?;
        let prepared = wgc_ipc::prepare_prefix(&drive_c, &RelayBinaries::locate()?)?;
        Some(prepared.relay_exe)
    } else {
        None
    };

    Ok(Prepared { runner, view, relay_exe })
}

fn account_responder(
    launch: LaunchId,
    sign_in: SignIn,
    language: GameLanguage,
    sender: &EventSender,
) -> AccountResponder {
    let SignIn { key, session, tracking_id, credentials } = sign_in;

    let sender = sender.clone();
    let observer: wgc_ipc::AccountObserver = Arc::new(move |event| {
        let event = match event {
            AccountEvent::Token2Saved => GameEvent::SignedIn { launch },
            AccountEvent::SessionRejected => GameEvent::SessionRejected { key },
        };
        sender.send(Event::Game(event));
    });
    let client = wgc_auth::AuthClient::new(key.realm, tracking_id);

    AccountResponder::new(session, client, credentials, language, observer)
}

/// Reports the client once the stub has started it, which means WGC checks passed.
fn watch_for_client(launch: LaunchId, handle: &GameHandle, sender: &EventSender) {
    const POLL: Duration = Duration::from_secs(1);

    loop {
        match handle.client_pid() {
            Ok(Some(_)) => {
                sender.send(Event::Game(GameEvent::ClientStarted { launch }));
                return;
            }
            Ok(None) => {}
            Err(err) => {
                tracing::warn!("could not look for the game client: {err}");
                return;
            }
        }

        match handle.is_running() {
            Ok(true) => std::thread::sleep(POLL),
            // The session ended before the client started; the exit reports it.
            Ok(false) => return,
            Err(err) => {
                tracing::warn!("could not check the game session: {err}");
                return;
            }
        }
    }
}

/// The content language the install was set up with for `realm`, which WGC reports to the
/// client.
fn game_language(view: &InstallView, realm: wgc_auth::AccountRealm) -> GameLanguage {
    // WGC's own default, which every WoWs realm ships.
    const DEFAULT: &str = "en";

    let read = view
        .files()
        .resolve_read(Path::new("game_info.xml"))
        .and_then(|path| path.map(std::fs::read_to_string).transpose());
    let language = match read {
        Ok(Some(text)) => content_language(&text, realm),
        Ok(None) => None,
        Err(err) => {
            tracing::warn!("could not read game_info.xml: {err}");
            None
        }
    };

    language.unwrap_or_else(|| {
        tracing::warn!("game_info.xml names no content language for {realm:?}; reporting {DEFAULT}");
        GameLanguage::new(DEFAULT)
    })
}

/// `<content_localization realm="eu">en</content_localization>` for `realm` in `game_info.xml`.
fn content_language(game_info: &str, realm: wgc_auth::AccountRealm) -> Option<GameLanguage> {
    let realm = match realm {
        wgc_auth::AccountRealm::Eu => "eu",
        wgc_auth::AccountRealm::Na => "na",
        wgc_auth::AccountRealm::Asia => "asia",
    };
    let open = format!("<content_localization realm=\"{realm}\">");

    let start = game_info.find(&open)? + open.len();
    let end = game_info[start..].find("</content_localization>")? + start;
    let language = game_info[start..end].trim();

    (!language.is_empty()).then(|| GameLanguage::new(language))
}

#[cfg(test)]
mod tests {
    use super::*;

    const GAME_INFO: &str = r#"<game>
        <localization>EN</localization>
        <content_localizations>
            <content_localization realm="asia">zh_sg</content_localization>
            <content_localization realm="eu">de</content_localization>
            <content_localization realm="na">en</content_localization>
        </content_localizations>
    </game>"#;

    #[test]
    fn the_content_language_is_the_realms() {
        let language = |realm| content_language(GAME_INFO, realm).map(|language| language.as_str().to_owned());

        assert_eq!(language(wgc_auth::AccountRealm::Eu).as_deref(), Some("de"));
        assert_eq!(language(wgc_auth::AccountRealm::Asia).as_deref(), Some("zh_sg"));
        assert!(content_language("<game/>", wgc_auth::AccountRealm::Na).is_none());
    }
}

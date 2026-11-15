//! Launches a WoWs install through the toolkit's runner, install view and WGC
//! relay, signed in as an account whose session is in the OS keyring. The
//! launcher's Play button follows the same steps.
//!
//! `play --data <dir> --realm eu --account <id> --tracking-id <tid> [--install <dir>]`
//!
//! The relay binaries come from `WOWS_TOOLKIT_WGC_RELAY_DIR`, else beside this
//! executable. `RUST_LOG=wgc_ipc=debug` logs every request and reply with
//! tokens redacted.

use std::path::Path;
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::AtomicU32;
use std::sync::atomic::Ordering;
use std::time::Duration;
use std::time::Instant;

use clap::Parser;
use clap::ValueEnum;
use tracing::info;
use tracing::warn;
use wgc_auth::AccountId;
use wgc_auth::AccountRealm;
use wgc_auth::AuthClient;
use wgc_auth::GameToken2;
use wgc_auth::SessionStore;
use wgc_core::TrackingId;
use wgc_ipc::AccountObserver;
use wgc_ipc::AccountResponder;
use wgc_ipc::BoxFuture;
use wgc_ipc::GameLanguage;
use wgc_ipc::IpcResponder;
use wgc_ipc::LoginInfo;
use wgc_ipc::RelayBinaries;
use wgc_ipc::RelayListener;
use wgc_ipc::Token1Failure;
use wgc_ipc::Token1Grant;
use wgc_ipc::Token2Grant;
use wgc_launch::InstallFiles;
use wgc_launch::InstallView;
use wgc_launch::LaunchArgs;
use wgc_launch::LaunchSpec;
use wgc_launch::Runner;
use wgc_launch::RunnerChoice;
use wgc_launch::RuntimeDirs;
use wgc_launch::WindowsProgram;
use wows_game_settings::GameSettingsStore;
use wows_game_settings::SettingsFiles;

#[derive(Parser)]
struct Args {
    /// The game install. Read-only installs run from an overlay.
    #[arg(long, default_value = "/mnt/e/WoWs/World_of_Warships")]
    install: PathBuf,
    /// The toolkit's app data directory, which holds the prefix and overlay state.
    #[arg(long)]
    data: PathBuf,
    #[arg(long, value_enum, default_value_t = RunnerArg::Auto)]
    runner: RunnerArg,
    #[arg(long, value_enum)]
    realm: RealmArg,
    #[arg(long)]
    account: u64,
    /// The toolkit's numeric tracking id, sent with every WGNI request.
    #[arg(long)]
    tracking_id: String,
    #[arg(long, default_value = "en")]
    language: String,
    /// Kills the game after this many seconds.
    #[arg(long, default_value_t = 300)]
    seconds: u64,
    /// Refuses token1 requests beyond this count.
    #[arg(long)]
    max_token1: Option<u32>,
    /// Writes Proton's log into this directory.
    #[arg(long)]
    proton_log: Option<PathBuf>,
}

#[derive(Clone, Copy, ValueEnum)]
enum RunnerArg {
    Auto,
    Umu,
}

#[derive(Clone, Copy, ValueEnum)]
enum RealmArg {
    Eu,
    Na,
    Asia,
}

impl From<RealmArg> for AccountRealm {
    fn from(realm: RealmArg) -> AccountRealm {
        match realm {
            RealmArg::Eu => AccountRealm::Eu,
            RealmArg::Na => AccountRealm::Na,
            RealmArg::Asia => AccountRealm::Asia,
        }
    }
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "info,wgc_ipc=debug,wgc_launch=debug".into()),
        )
        .init();
    let args = Args::parse();
    let realm = AccountRealm::from(args.realm);
    let account_id = AccountId::new(args.account);
    let tracking_id = TrackingId::parse(&args.tracking_id)?;

    let dirs = RuntimeDirs::new(&args.data);
    let choice = match args.runner {
        RunnerArg::Auto => RunnerChoice::Auto,
        RunnerArg::Umu => RunnerChoice::Umu { umu_run: None, proton_dir: None },
    };
    let runner = Runner::detect(&choice, &dirs)?;
    info!(runner = ?runner.runner(), first_run = ?runner.first_run_work(), "runner resolved");
    if !runner.prefix().is_initialized() {
        let started = Instant::now();
        runner.initialize_prefix()?;
        info!(elapsed = ?started.elapsed(), "prefix initialized");
    }

    let view = InstallView::prepare(&args.install, &dirs)?;
    info!(?view, "install view prepared");

    let drive_c = runner.prefix().drive_c().ok_or("the runner has no Wine prefix")?;
    let prepared = wgc_ipc::prepare_prefix(&drive_c, &RelayBinaries::locate()?)?;

    let runtime = tokio::runtime::Builder::new_multi_thread().enable_all().build()?;
    let responder = runtime.block_on(responder(realm, account_id, tracking_id, GameLanguage::new(args.language)));
    let responder: Arc<dyn IpcResponder> = match args.max_token1 {
        Some(limit) => Arc::new(Token1Limit { inner: responder, remaining: AtomicU32::new(limit) }),
        None => responder,
    };

    let listener = runtime.block_on(RelayListener::bind())?;
    let program = WindowsProgram {
        exe: prepared.relay_exe,
        args: vec![wgc_launch::dos_path(&view.game_exe())],
        env: listener.relay_env(),
        shared_dirs: Vec::new(),
    };
    let serving = runtime.spawn(listener.serve(responder));

    let mut launch_args = LaunchArgs::default();
    if let Some(dir) = &args.proton_log {
        launch_args.env.push(("PROTON_LOG".into(), "1".into()));
        launch_args.env.push(("PROTON_LOG_DIR".into(), dir.display().to_string()));
    }
    let process = wgc_launch::spawn(LaunchSpec { runner: &runner, view: &view, program, args: &launch_args }, |_| {})?;
    let handle = process.handle();
    info!(runner_pid = ?handle.runner_pid(), "game started");

    // The game runs until it exits, the time limit passes, or the game loses
    // its WGC answers.
    let waiter = std::thread::spawn(move || process.wait());
    let deadline = Instant::now() + Duration::from_secs(args.seconds);
    while !waiter.is_finished() && !serving.is_finished() && Instant::now() < deadline {
        std::thread::sleep(Duration::from_millis(500));
    }

    let killed = if waiter.is_finished() {
        Ok(())
    } else {
        info!(listener_stopped = serving.is_finished(), "killing the game");
        handle.kill()
    };

    // Reaps the runner and unmounts the overlay even when the kill failed.
    let exit = waiter.join().map_err(|_| "wait thread panicked")?;
    killed?;
    let exit = exit?;
    info!(?exit, "game exited");

    if serving.is_finished() {
        runtime.block_on(serving)??;
    }

    let files = view.files();
    for rel in ["profile/clientrunner.log", "profile/python.log", "preferences.xml"] {
        info!(rel, path = ?files.resolve_read(Path::new(rel))?, "game file");
    }

    // Settings the game wrote live in the view's upper layer.
    let snapshot = GameSettingsStore::new(ViewSettingsFiles(files), None).read()?;
    info!(display = ?snapshot.settings().display, "game settings read through the view");

    Ok(())
}

/// The game settings files of an install, through its view.
struct ViewSettingsFiles(InstallFiles);

impl SettingsFiles for ViewSettingsFiles {
    fn read_path(&self, relative: &Path) -> std::io::Result<Option<PathBuf>> {
        self.0.resolve_read(relative)
    }

    fn write_path(&self, relative: &Path) -> std::io::Result<PathBuf> {
        self.0.resolve_write(relative)
    }
}

/// The stored session's account, or a signed-out answer when the keyring
/// holds none, so the game's behavior without an account can be observed.
async fn responder(
    realm: AccountRealm,
    account_id: AccountId,
    tracking_id: TrackingId,
    language: GameLanguage,
) -> Arc<dyn IpcResponder> {
    let loaded = tokio::task::spawn_blocking(move || SessionStore::new().load(realm, account_id)).await;
    match loaded {
        Ok(Ok(session)) => {
            info!(nickname = session.nickname.as_str(), "session loaded");
            let client = AuthClient::new(session.realm, tracking_id);
            let observer: AccountObserver = Arc::new(|event| info!(?event, "account event"));
            Arc::new(AccountResponder::new(Arc::new(session), client, language, observer))
        }
        Ok(Err(err)) => {
            warn!(%err, "no stored session, answering as signed out");
            Arc::new(SignedOut { realm, language })
        }
        Err(err) => {
            warn!(%err, "session load task failed, answering as signed out");
            Arc::new(SignedOut { realm, language })
        }
    }
}

struct SignedOut {
    realm: AccountRealm,
    language: GameLanguage,
}

impl IpcResponder for SignedOut {
    fn client_realm(&self) -> AccountRealm {
        self.realm
    }

    fn login_info(&self) -> Option<LoginInfo> {
        None
    }

    fn game_language(&self) -> GameLanguage {
        self.language.clone()
    }

    fn query_token1(&self) -> BoxFuture<'_, Result<Token1Grant, Token1Failure>> {
        Box::pin(async { Err(Token1Failure::NoUser) })
    }

    fn load_token2(&self) -> BoxFuture<'_, Option<Token2Grant>> {
        Box::pin(async { None })
    }

    fn save_token2(&self, _spa_id: AccountId, _token2: GameToken2) -> BoxFuture<'_, ()> {
        Box::pin(async {})
    }
}

/// Caps token1 mints per launch; WGNI rate-limits them.
struct Token1Limit {
    inner: Arc<dyn IpcResponder>,
    remaining: AtomicU32,
}

impl IpcResponder for Token1Limit {
    fn client_realm(&self) -> AccountRealm {
        self.inner.client_realm()
    }

    fn login_info(&self) -> Option<LoginInfo> {
        self.inner.login_info()
    }

    fn game_language(&self) -> GameLanguage {
        self.inner.game_language()
    }

    fn query_token1(&self) -> BoxFuture<'_, Result<Token1Grant, Token1Failure>> {
        let granted = self.remaining.try_update(Ordering::SeqCst, Ordering::SeqCst, |left| left.checked_sub(1));
        if granted.is_err() {
            warn!("token1 limit reached, refusing the request");
            return Box::pin(async { Err(Token1Failure::Cancelled) });
        }
        self.inner.query_token1()
    }

    fn load_token2(&self) -> BoxFuture<'_, Option<Token2Grant>> {
        self.inner.load_token2()
    }

    fn save_token2(&self, spa_id: AccountId, token2: GameToken2) -> BoxFuture<'_, ()> {
        self.inner.save_token2(spa_id, token2)
    }
}

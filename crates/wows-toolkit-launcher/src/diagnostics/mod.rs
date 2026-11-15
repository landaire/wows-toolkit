//! Dev builds only: what the game's processes do in the first minute after the client signs in,
//! to catch the intermittent stall after login in which the client's HTTPS requests time out and
//! its UDP channel drops. Each launch writes `<game runtime>/launch-diagnostics/<unix secs>-<launch>/`.
//! The capture runs on its own threads; its failures are logged and never reach the launch.

mod dns;
mod logs;
mod output;
mod procfs;
mod stacks;

use std::collections::HashMap;
use std::fmt;
use std::fmt::Write as _;
use std::path::Path;
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::Condvar;
use std::sync::Mutex;
use std::sync::PoisonError;
use std::sync::mpsc;
use std::thread::JoinHandle;
use std::time::Duration;
use std::time::Instant;

use wgc_ipc::AccountEvent;
use wgc_ipc::AccountObserver;
use wgc_launch::GameHandle;
use wgc_launch::InstallFiles;
use wgc_launch::ProcessId;
use wgc_launch::RuntimeDirs;
use wgc_launch::SessionMember;

use crate::diagnostics::output::ByteLimit;
use crate::diagnostics::output::CaptureFile;
use crate::diagnostics::output::CaptureWriter;
use crate::diagnostics::output::LaunchBudget;
use crate::diagnostics::output::LaunchFolder;
use crate::diagnostics::output::UnixTime;
use crate::diagnostics::procfs::SocketEntry;
use crate::diagnostics::procfs::SocketInode;
use crate::diagnostics::procfs::SyscallKind;
use crate::diagnostics::procfs::ThreadId;
use crate::diagnostics::procfs::ThreadStat;
use crate::diagnostics::procfs::ThreadSyscall;
use crate::error::CaptureError;
use crate::game::LaunchId;

const CLIENT_EXE: &str = "WorldOfWarships64.exe";
const WINESERVER_EXE: &str = "wineserver";

/// Starts the capture when the client saves its token2, which is when it logs `LOGGED_ON`.
pub(crate) struct SignInTrigger(mpsc::Sender<Instant>);

impl SignInTrigger {
    /// `observer`, also starting the capture.
    pub(crate) fn observe(self, observer: AccountObserver) -> AccountObserver {
        Arc::new(move |event| {
            if matches!(event, AccountEvent::Token2Saved) {
                // A later save finds the capture started or over.
                let _ = self.0.send(Instant::now());
            }
            observer(event);
        })
    }
}

/// A capture waiting for the sign-in.
pub(crate) struct PendingCapture {
    launch: LaunchId,
    root: PathBuf,
    files: InstallFiles,
    signed_in: mpsc::Receiver<Instant>,
}

/// Time since the client signed in.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
struct SinceSignIn(Duration);

impl fmt::Display for SinceSignIn {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{:.3}", self.0.as_secs_f64())
    }
}

#[derive(Debug, Clone, Copy)]
struct Clock {
    signed_in: Instant,
}

impl Clock {
    fn now(&self) -> SinceSignIn {
        SinceSignIn(self.signed_in.elapsed())
    }
}

/// Tells the sampling threads to finish.
#[derive(Debug, Clone)]
struct StopSignal(Arc<(Mutex<bool>, Condvar)>);

impl StopSignal {
    fn new() -> Self {
        Self(Arc::new((Mutex::new(false), Condvar::new())))
    }

    fn stop(&self) {
        let (stopped, wake) = &*self.0;
        *stopped.lock().unwrap_or_else(PoisonError::into_inner) = true;
        wake.notify_all();
    }

    /// Sleeps for `duration` or until stopped. True once stopped.
    fn wait(&self, duration: Duration) -> bool {
        let (stopped, wake) = &*self.0;
        let guard = stopped.lock().unwrap_or_else(PoisonError::into_inner);
        let (guard, _) =
            wake.wait_timeout_while(guard, duration, |stopped| !*stopped).unwrap_or_else(PoisonError::into_inner);

        *guard
    }
}

/// A process the capture samples.
#[derive(Debug, Clone)]
struct Target {
    pid: ProcessId,
    label: &'static str,
}

/// Why the capture ended.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum CaptureEnd {
    Window,
    ClientExited,
}

impl fmt::Display for CaptureEnd {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Window => write!(f, "the window ended"),
            Self::ClientExited => write!(f, "the client exited"),
        }
    }
}

/// The open data files of the network sampler.
struct NetworkFiles {
    syscalls: CaptureWriter,
    sockets: CaptureWriter,
}

/// The IP sockets of a network namespace, read once per sampling round on first use.
struct SocketTable {
    namespace_of: ProcessId,
    entries: Option<HashMap<SocketInode, SocketEntry>>,
}

impl SocketTable {
    fn new(namespace_of: ProcessId) -> Self {
        Self { namespace_of, entries: None }
    }

    fn entries(&mut self) -> &HashMap<SocketInode, SocketEntry> {
        self.entries.get_or_insert_with(|| procfs::socket_table(self.namespace_of))
    }
}

type Worker = JoinHandle<Result<(), CaptureError>>;

/// A capture for `launch`, writing under `dirs`, that reads the game's logs through `files`.
pub(crate) fn prepare(launch: LaunchId, dirs: &RuntimeDirs, files: InstallFiles) -> (SignInTrigger, PendingCapture) {
    let (trigger, signed_in) = mpsc::channel();
    let root = dirs.root().join("launch-diagnostics");

    (SignInTrigger(trigger), PendingCapture { launch, root, files, signed_in })
}

/// Runs `pending` on its own thread once the client of `handle` signs in.
pub(crate) fn spawn(pending: PendingCapture, handle: GameHandle) {
    let spawned = std::thread::Builder::new().name("launch-diagnostics".to_owned()).spawn(move || {
        let Some(signed_in) = wait_for_sign_in(&pending.signed_in, &handle) else {
            return;
        };
        if let Err(err) = capture(pending, &handle, signed_in) {
            tracing::warn!("launch diagnostics stopped: {err}");
        }
    });

    if let Err(source) = spawned {
        tracing::warn!("{}", CaptureError::Thread { name: "launch-diagnostics", source });
    }
}

/// The time the client signed in, or `None` once the session ends without it.
fn wait_for_sign_in(signed_in: &mpsc::Receiver<Instant>, handle: &GameHandle) -> Option<Instant> {
    const POLL: Duration = Duration::from_secs(5);

    loop {
        match signed_in.recv_timeout(POLL) {
            Ok(at) => return Some(at),
            // The relay stopped serving, so the session is over.
            Err(mpsc::RecvTimeoutError::Disconnected) => return None,
            Err(mpsc::RecvTimeoutError::Timeout) => {}
        }

        match handle.is_running() {
            Ok(true) => {}
            Ok(false) => return None,
            Err(err) => {
                tracing::warn!("launch diagnostics could not check the game session: {err}");
                return None;
            }
        }
    }
}

fn capture(pending: PendingCapture, handle: &GameHandle, signed_in: Instant) -> Result<(), CaptureError> {
    const LAUNCH_LIMIT: ByteLimit = ByteLimit::mib(16);

    let PendingCapture { launch, root, files, signed_in: _ } = pending;
    let clock = Clock { signed_in };
    let started = UnixTime::of(signed_in)?;

    let folder = LaunchFolder::create(&root, launch, started)?;
    tracing::info!("recording launch diagnostics in {}", folder.path().display());
    let budget = LaunchBudget::new(LAUNCH_LIMIT);
    let mut notes = folder.open(CaptureFile::Capture, &budget)?;
    notes.record(format!("signed in at unix {} ms; times are seconds since then\n", started.millis()).as_bytes())?;

    let members = handle.members().map_err(|source| CaptureError::Members { source })?;
    let mut listing = String::from("session processes:\n");
    for member in &members {
        let _ = writeln!(listing, "  {} {}", member.pid.0, member.exe_name.as_deref().unwrap_or("?"));
    }
    notes.record(listing.as_bytes())?;

    let client = member_named(&members, CLIENT_EXE).ok_or(CaptureError::NoProcess { exe: CLIENT_EXE })?;
    let mut targets = vec![Target { pid: client, label: CLIENT_EXE }];
    match member_named(&members, WINESERVER_EXE) {
        Some(pid) => targets.push(Target { pid, label: WINESERVER_EXE }),
        None => warn(&mut notes, &clock, &CaptureError::NoProcess { exe: WINESERVER_EXE }),
    }

    let main = busiest_thread(client);
    notes.record(
        format!("main thread: {}\n", main.map_or_else(|| "unknown".to_owned(), |tid| tid.to_string())).as_bytes(),
    )?;

    if let Err(err) = copy_maps(client, &folder, &budget) {
        warn(&mut notes, &clock, &err);
    }

    // Every file opens before a worker starts, so a failed open cannot leave one running.
    let stacks_out = folder.open(CaptureFile::Stacks, &budget)?;
    let dns_out = folder.open(CaptureFile::Dns, &budget)?;
    let network = NetworkFiles {
        syscalls: folder.open(CaptureFile::Syscalls, &budget)?,
        sockets: folder.open(CaptureFile::Sockets, &budget)?,
    };

    let stop = StopSignal::new();
    let mut workers = Vec::new();

    let stack_targets = targets.clone();
    let stack_stop = stop.clone();
    match start_worker("launch-diag-stacks", move || stacks::sample(&stack_targets, stacks_out, &clock, &stack_stop)) {
        Ok(worker) => workers.push(worker),
        Err(err) => warn(&mut notes, &clock, &err),
    }

    match python_log(&files) {
        Ok(path) => {
            let dns_stop = stop.clone();
            match start_worker("launch-diag-dns", move || dns::probe(path, dns_out, &clock, &dns_stop)) {
                Ok(worker) => workers.push(worker),
                Err(err) => warn(&mut notes, &clock, &err),
            }
        }
        Err(err) => warn(&mut notes, &clock, &err),
    }

    let ended = sample_network(&targets, main, network, &clock);

    stop.stop();
    for worker in workers {
        match worker.join() {
            Ok(Ok(())) => {}
            Ok(Err(err)) => warn(&mut notes, &clock, &err),
            Err(_) => warn(&mut notes, &clock, &CaptureError::WorkerPanicked),
        }
    }
    let end = ended?;

    for (rel, file) in [("profile/python.log", CaptureFile::PythonLog), ("profile/debug.log", CaptureFile::DebugLog)] {
        if let Err(err) = copy_log_tail(&files, Path::new(rel), &folder, file, &budget) {
            warn(&mut notes, &clock, &err);
        }
    }

    notes.record(format!("{} capture ended: {end}\n", clock.now()).as_bytes())?;
    tracing::info!("finished launch diagnostics in {}: {end}", folder.path().display());

    Ok(())
}

/// Logs `err` and notes it in capture.txt; the capture goes on without that part.
fn warn(notes: &mut CaptureWriter, clock: &Clock, err: &CaptureError) {
    tracing::warn!("launch diagnostics: {err}");

    // The warning is logged already; a failed note has nowhere else to go.
    let _ = notes.record(format!("{} warning: {err}\n", clock.now()).as_bytes());
}

fn start_worker(
    name: &'static str,
    work: impl FnOnce() -> Result<(), CaptureError> + Send + 'static,
) -> Result<Worker, CaptureError> {
    std::thread::Builder::new()
        .name(name.to_owned())
        .spawn(work)
        .map_err(|source| CaptureError::Thread { name, source })
}

fn member_named(members: &[SessionMember], exe: &str) -> Option<ProcessId> {
    members
        .iter()
        .find(|member| member.exe_name.as_deref().is_some_and(|name| name.eq_ignore_ascii_case(exe)))
        .map(|member| member.pid)
}

/// The client thread named after the game with the most CPU time so far: the game's main
/// thread, which sits idle during the stall.
fn busiest_thread(client: ProcessId) -> Option<ThreadId> {
    const GAME_THREAD: &str = "WorldOfWarships";

    procfs::threads(client)
        .ok()?
        .into_iter()
        .filter_map(|tid| read_thread_stat(client, tid).map(|stat| (tid, stat)))
        .filter(|(_, stat)| stat.comm.starts_with(GAME_THREAD))
        .max_by_key(|(_, stat)| stat.cpu)
        .map(|(tid, _)| tid)
}

fn read_thread_stat(pid: ProcessId, tid: ThreadId) -> Option<ThreadStat> {
    ThreadStat::parse(&std::fs::read_to_string(format!("/proc/{}/task/{tid}/stat", pid.0)).ok()?)
}

fn copy_maps(client: ProcessId, folder: &LaunchFolder, budget: &LaunchBudget) -> Result<(), CaptureError> {
    let path = PathBuf::from(format!("/proc/{}/maps", client.0));
    let maps = std::fs::read(&path).map_err(|source| CaptureError::Read { path, source })?;

    // One record per mapping, so the file limit keeps a prefix rather than dropping all of it.
    let mut out = folder.open(CaptureFile::Maps, budget)?;
    for line in maps.split_inclusive(|&byte| byte == b'\n') {
        out.record(line)?;
    }

    Ok(())
}

fn python_log(files: &InstallFiles) -> Result<PathBuf, CaptureError> {
    resolve_log(files, Path::new("profile/python.log"))
}

fn resolve_log(files: &InstallFiles, rel: &Path) -> Result<PathBuf, CaptureError> {
    files
        .resolve_read(rel)
        .map_err(|source| CaptureError::Read { path: rel.to_owned(), source })?
        .ok_or_else(|| CaptureError::NoLog { path: rel.to_owned() })
}

fn copy_log_tail(
    files: &InstallFiles,
    rel: &Path,
    folder: &LaunchFolder,
    file: CaptureFile,
    budget: &LaunchBudget,
) -> Result<(), CaptureError> {
    let path = resolve_log(files, rel)?;
    let tail = logs::tail(&path, file.limit().0).map_err(|source| CaptureError::Read { path, source })?;

    folder.open(file, budget)?.record(&tail)
}

/// Samples the blocking system calls of the targets' threads and the client's sockets until the
/// window ends or the client exits.
fn sample_network(
    targets: &[Target],
    main: Option<ThreadId>,
    mut files: NetworkFiles,
    clock: &Clock,
) -> Result<CaptureEnd, CaptureError> {
    const WINDOW: Duration = Duration::from_secs(60);
    const INTERVAL: Duration = Duration::from_millis(250);
    const SOCKET_ROUNDS: u32 = 4;

    let Some(client) = targets.first().map(|target| target.pid) else {
        return Ok(CaptureEnd::ClientExited);
    };
    files.syscalls.record(b"t\tpid\ttid\tcomm\tstate\tcpu_ticks\tsyscall\tfd\twchan\n")?;
    files.sockets.record(b"t\tsocket\n")?;

    let mut round = 0u32;
    loop {
        let at = clock.now();
        if at.0 >= WINDOW {
            return Ok(CaptureEnd::Window);
        }
        if !Path::new(&format!("/proc/{}", client.0)).exists() {
            return Ok(CaptureEnd::ClientExited);
        }

        let started = Instant::now();
        let mut sockets = SocketTable::new(client);
        let mut rows = String::new();
        for target in targets {
            thread_rows(target, main, at, &mut sockets, &mut rows);
        }
        files.syscalls.record(rows.as_bytes())?;

        if round.is_multiple_of(SOCKET_ROUNDS) {
            files.sockets.record(socket_rows(client, at, sockets.entries()).as_bytes())?;
        }
        round = round.wrapping_add(1);

        std::thread::sleep(INTERVAL.saturating_sub(started.elapsed()));
    }
}

/// Rows for the threads of `target` that are the main thread, in a network call, or reading or
/// writing a socket.
fn thread_rows(target: &Target, main: Option<ThreadId>, at: SinceSignIn, sockets: &mut SocketTable, rows: &mut String) {
    let threads = match procfs::threads(target.pid) {
        Ok(threads) => threads,
        Err(err) => {
            let _ = writeln!(rows, "{at}\t{}\terror: {err}", target.pid.0);
            return;
        }
    };

    for tid in threads {
        let task = format!("/proc/{}/task/{tid}", target.pid.0);
        // Threads exit between listing and reading.
        let Some(syscall) =
            std::fs::read_to_string(format!("{task}/syscall")).ok().and_then(|text| ThreadSyscall::parse(&text))
        else {
            continue;
        };
        let Some(stat) = read_thread_stat(target.pid, tid) else {
            continue;
        };

        let link = syscall.fd().and_then(|fd| procfs::fd_target(target.pid, fd));
        let inode = link.as_deref().and_then(Path::to_str).and_then(procfs::socket_inode);
        let is_main = main == Some(tid);
        let keep = is_main
            || match syscall.kind() {
                SyscallKind::Network => true,
                SyscallKind::FdIo => inode.is_some(),
                SyscallKind::Other => false,
            };
        if !keep {
            continue;
        }

        let fd = match (link, inode) {
            (Some(link), Some(inode)) => match sockets.entries().get(&inode) {
                Some(entry) => format!("{} {entry}", link.display()),
                None => link.display().to_string(),
            },
            (Some(link), None) => link.display().to_string(),
            (None, _) => String::new(),
        };
        let wchan = std::fs::read_to_string(format!("{task}/wchan")).unwrap_or_else(|err| format!("error: {err}"));
        let marker = if is_main { "*" } else { "" };

        let _ = writeln!(
            rows,
            "{at}\t{}\t{tid}{marker}\t{}\t{}\t{}\t{syscall}\t{fd}\t{wchan}",
            target.pid.0, stat.comm, stat.state, stat.cpu,
        );
    }
}

/// One row per socket of `client`.
fn socket_rows(client: ProcessId, at: SinceSignIn, table: &HashMap<SocketInode, SocketEntry>) -> String {
    let inodes = match procfs::socket_inodes(client) {
        Ok(inodes) => inodes,
        Err(err) => return format!("{at}\terror: {err}\n"),
    };

    let mut rows = String::new();
    for inode in inodes {
        // Unix sockets have no row in the IP tables.
        if let Some(entry) = table.get(&inode) {
            let _ = writeln!(rows, "{at}\t{entry}");
        }
    }

    rows
}

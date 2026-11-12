//! Reads replays in a child process.
//!
//! A replay is untrusted input, and parsing one runs a large decoder over it.
//! The worker is the host program itself, started again with [`WORKER_ENV`]
//! set, an empty environment and an empty working directory: call [`init`]
//! first in `main` of every program that reads replays. It reads the
//! parent's request and loads the game data the request names. Only then
//! does it cap its address space, deny itself filesystem access with
//! Landlock on Linux and lower its descriptor limit so that it can open no
//! file or socket, and read the replay. The parent kills it at a deadline.
//!
//! The parent writes one JSON request line, then the replay as a 4-byte
//! little-endian length and its bytes. The worker answers with JSON
//! [`Reply`] lines.

use std::io::BufRead;
use std::io::BufReader;
use std::io::BufWriter;
use std::io::Read;
use std::io::Write;
use std::ops::ControlFlow;
use std::path::Path;
use std::path::PathBuf;
use std::process::Command;
use std::process::Stdio;
use std::sync::OnceLock;
use std::sync::atomic::AtomicBool;
use std::sync::atomic::AtomicU64;
use std::sync::atomic::Ordering;
use std::sync::mpsc;
use std::sync::mpsc::RecvTimeoutError;
use std::time::Duration;
use std::time::Instant;

use replayshark::LoadedBuild;
use replayshark::PacketFilter;
use replayshark::PacketRecord;
use replayshark::PacketScan;
use replayshark::ReplayAnalysis;
use serde::Deserialize;
use serde::Serialize;
use wows_replays::ReplayFile;
use wows_replays::game_constants::GameConstants;
use wowsunpack::data::Version;

use crate::data::ReplayGameData;
use crate::error::ReplayWorkerError;
use crate::sandbox::SandboxError;
use crate::sandbox::WorkerSlots;

/// Set in the environment of a worker process.
pub const WORKER_ENV: &str = "WOWS_QUERY_REPLAY_WORKER";

/// Largest replay a worker reads.
pub const MAX_REPLAY_BYTES: ByteCount = ByteCount::from_mib(64);

static WORKER_PROGRAM: OnceLock<PathBuf> = OnceLock::new();

/// One worker at a time: each loads a whole build's game data.
static WORKER_SLOTS: WorkerSlots = WorkerSlots::new(1);

/// Distinguishes the working directories of workers.
static NEXT_WORKDIR: AtomicU64 = AtomicU64::new(0);

/// A game data build the worker loads before it reads the replay.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BuildSource {
    /// A dump build directory, holding `metadata.toml`.
    pub dump_dir: PathBuf,

    /// Where rebuilt `game_params.rkyv` files are kept, as for
    /// [`crate::GameData::load_dump`].
    pub cache_root: Option<PathBuf>,
}

/// How many matching packets a worker returns. It counts every match.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct PacketLimits {
    pub records: RecordCount,

    /// Total size of the returned records as compact JSON.
    pub bytes: ByteCount,
}

/// A number of packet records.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct RecordCount(usize);

impl RecordCount {
    pub const fn new(records: usize) -> Self {
        Self(records)
    }

    pub const fn get(self) -> usize {
        self.0
    }
}

/// A size in bytes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct ByteCount(usize);

impl ByteCount {
    pub const fn new(bytes: usize) -> Self {
        Self(bytes)
    }

    pub const fn from_mib(mib: usize) -> Self {
        Self(mib * 1024 * 1024)
    }

    pub const fn get(self) -> usize {
        self.0
    }
}

/// The replay's plaintext header: what choosing game data for it needs.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ReplayHeader {
    /// `None` when the header's client version does not parse.
    pub version: Option<Version>,

    /// As written, such as `15,9,0,13357625`.
    pub client_version: String,
    pub player: String,
    pub player_ship: String,

    /// The space name, such as `spaces/41_Conquest`.
    pub map: String,
    pub game_type: Option<String>,
    pub date_time: String,
}

/// Matching packets, up to the limits, and what the scan covered.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PacketSelection {
    pub records: Vec<PacketRecord>,
    pub scan: PacketScan,
}

/// Why a worker could not answer.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, thiserror::Error)]
#[serde(rename_all = "snake_case")]
pub enum WorkerFailure {
    #[error("failed to load game data from {dump_dir}: {message}")]
    GameData { dump_dir: PathBuf, message: String },

    #[error("the replay is {} bytes, more than the {} byte limit", bytes.get(), limit.get())]
    TooLarge { bytes: ByteCount, limit: ByteCount },

    #[error("not a readable replay: {message}")]
    Replay { message: String },

    #[error("failed to analyze the replay: {message}")]
    Analysis { message: String },

    #[error("the replay worker failed while reading the replay")]
    Panicked,
}

/// What a worker process denied itself before it read the replay.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Confinement {
    pub filesystem: Restriction,
    pub descriptors: Restriction,
    pub processes: Restriction,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Restriction {
    Applied,
    Unavailable { reason: String },
}

impl Restriction {
    fn from_result(result: Result<(), SandboxError>) -> Self {
        match result {
            Ok(()) => Self::Applied,
            Err(error) => Self::Unavailable { reason: error.reason() },
        }
    }
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(tag = "op", rename_all = "snake_case")]
enum Request {
    Header,
    Analyze { build: BuildSource },
    Packets { build: BuildSource, filter: PacketFilter, limits: PacketLimits },
}

impl Request {
    /// How long the worker may take, game data loading included.
    fn time_limit(&self) -> Duration {
        match self {
            Self::Header => Duration::from_secs(10),
            Self::Analyze { .. } | Self::Packets { .. } => Duration::from_secs(120),
        }
    }
}

/// A request with its game data loaded.
enum Job {
    Header,
    Analyze(Loaded),
    Packets { loaded: Loaded, filter: PacketFilter, limits: PacketLimits },
}

impl Job {
    fn prepare(request: Request) -> Result<Self, WorkerFailure> {
        match request {
            Request::Header => Ok(Self::Header),
            Request::Analyze { build } => Ok(Self::Analyze(load_build(&build)?)),
            Request::Packets { build, filter, limits } => {
                Ok(Self::Packets { loaded: load_build(&build)?, filter, limits })
            }
        }
    }
}

/// Game data the worker parses with, and the build's wows-constants JSON
/// that names its battle results fields.
struct Loaded {
    build: LoadedBuild,
    results_constants: Option<serde_json::Value>,
}

enum Collected {
    Replies(Vec<Reply>),
    Failed(WorkerFailure),
    TimedOut,
    /// The replies ended before one completed the request.
    Ended,
    Unreadable(serde_json::Error),
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
enum Reply {
    /// The worker confined itself; sent before it reads the replay.
    Started(Confinement),
    Header(ReplayHeader),
    Analysis(Box<ReplayAnalysis>),
    Packet(PacketRecord),
    Scanned(PacketScan),
    Failed(WorkerFailure),
}

/// In a worker process, answers the request on standard input and exits.
/// Otherwise registers this program as the replay worker. Call it first in
/// `main`, before anything that reads standard input or starts a thread.
pub fn init() {
    if std::env::var_os(WORKER_ENV).is_some() {
        let code = match serve() {
            Ok(()) => 0,
            Err(_) => 1,
        };
        std::process::exit(code);
    }

    match std::env::current_exe() {
        Ok(program) => set_worker_program(program),
        Err(error) => tracing::warn!(%error, "cannot locate this program; replays cannot be read"),
    }
}

/// Uses `program` as the replay worker: a program whose `main` calls
/// [`init`] first. The first call wins.
pub fn set_worker_program(program: PathBuf) {
    // The worker starts in its own working directory, where a relative path
    // names nothing. Only an empty path fails, and it names nothing either.
    let program = std::path::absolute(&program).unwrap_or(program);
    // A later call names the same kind of worker; keeping the first is correct.
    let _ = WORKER_PROGRAM.set(program);
}

/// Reads `replay`'s header without game data.
pub fn header(replay: &[u8]) -> Result<ReplayHeader, ReplayWorkerError> {
    let replies = run(&Request::Header, replay)?;

    match replies.into_iter().next() {
        Some(Reply::Header(header)) => Ok(header),
        _ => Err(ReplayWorkerError::UnexpectedReply),
    }
}

/// Parses `replay` with `build`'s game data.
pub fn analyze(replay: &[u8], build: &BuildSource) -> Result<ReplayAnalysis, ReplayWorkerError> {
    let replies = run(&Request::Analyze { build: absolute(build) }, replay)?;

    match replies.into_iter().next() {
        Some(Reply::Analysis(analysis)) => Ok(*analysis),
        _ => Err(ReplayWorkerError::UnexpectedReply),
    }
}

/// Decodes `replay`'s packets with `build`'s game data and returns those
/// `filter` admits, up to `limits`.
pub fn packets(
    replay: &[u8],
    build: &BuildSource,
    filter: &PacketFilter,
    limits: PacketLimits,
) -> Result<PacketSelection, ReplayWorkerError> {
    let request = Request::Packets { build: absolute(build), filter: filter.clone(), limits };
    let mut records = Vec::new();
    for reply in run(&request, replay)? {
        match reply {
            Reply::Packet(record) => records.push(record),
            Reply::Scanned(scan) => return Ok(PacketSelection { records, scan }),
            _ => return Err(ReplayWorkerError::UnexpectedReply),
        }
    }

    Err(ReplayWorkerError::UnexpectedReply)
}

/// `build` with absolute paths: the worker runs in its own working
/// directory, where a relative path names nothing. Only an empty path
/// fails, and it names nothing either.
fn absolute(build: &BuildSource) -> BuildSource {
    let absolute = |path: &PathBuf| std::path::absolute(path).unwrap_or_else(|_| path.clone());

    BuildSource { dump_dir: absolute(&build.dump_dir), cache_root: build.cache_root.as_ref().map(absolute) }
}

/// Runs one worker and returns its replies after [`Reply::Started`], ending
/// with the reply that completes the request.
fn run(request: &Request, replay: &[u8]) -> Result<Vec<Reply>, ReplayWorkerError> {
    // Two analyses of a large replay ahead of the caller.
    const MAX_WAIT: Duration = Duration::from_secs(240);

    let bytes = ByteCount::new(replay.len());
    if bytes > MAX_REPLAY_BYTES {
        return Err(ReplayWorkerError::TooLarge { bytes, limit: MAX_REPLAY_BYTES });
    }
    let Some(worker) = WORKER_PROGRAM.get() else {
        return Err(ReplayWorkerError::NoWorker);
    };
    let Some(_permit) = WORKER_SLOTS.acquire(MAX_WAIT) else {
        return Err(ReplayWorkerError::Busy { waited: MAX_WAIT });
    };

    // An empty working directory, so nothing the host keeps there is reachable.
    let workdir = std::env::temp_dir().join(format!(
        "wows-query-replay-{}-{}",
        std::process::id(),
        NEXT_WORKDIR.fetch_add(1, Ordering::Relaxed)
    ));
    std::fs::create_dir_all(&workdir).map_err(|source| ReplayWorkerError::Workdir { path: workdir.clone(), source })?;
    let mut command = Command::new(worker);
    // The worker must not see the host's secrets.
    command.env_clear().env(WORKER_ENV, "1").current_dir(&workdir);
    // Windows programs fail to start some system services without it.
    #[cfg(windows)]
    if let Some(system_root) = std::env::var_os("SystemRoot") {
        command.env("SystemRoot", system_root);
    }
    // Its own process group, so a kill also ends anything it started.
    #[cfg(unix)]
    std::os::unix::process::CommandExt::process_group(&mut command, 0);
    let spawned = command.stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(Stdio::null()).spawn();
    let mut child = match spawned {
        Ok(child) => child,
        Err(source) => {
            let _ = std::fs::remove_dir(&workdir);
            return Err(ReplayWorkerError::Spawn { program: worker.clone(), source });
        }
    };
    let stdin = child.stdin.take().expect("stdin is piped");
    let stdout = child.stdout.take().expect("stdout is piped");

    let limit = request.time_limit();
    let deadline = Instant::now() + limit;
    let outcome = std::thread::scope(|scope| {
        let (sender, receiver) = mpsc::channel();
        scope.spawn(move || write_request(stdin, request, replay));
        scope.spawn(move || read_replies(stdout, &sender));

        let outcome = collect_replies(&receiver, deadline, limit);
        // Ends a worker past its deadline, and with it both pipe threads.
        kill(&mut child);

        outcome
    });
    let status = child.wait().ok();
    // The directory stays empty; a failure leaves an empty directory behind.
    let _ = std::fs::remove_dir(&workdir);

    match outcome {
        Collected::Replies(replies) => Ok(replies),
        Collected::Failed(failure) => Err(ReplayWorkerError::Failed(failure)),
        Collected::TimedOut => Err(ReplayWorkerError::TimedOut { limit }),
        Collected::Ended => Err(ReplayWorkerError::Crashed { status }),
        Collected::Unreadable(error) => Err(ReplayWorkerError::Reply(error)),
    }
}

/// Kills the worker and, on Unix, its process group: a process it started
/// could otherwise hold its stdout open and keep the reader waiting.
fn kill(child: &mut std::process::Child) {
    #[cfg(unix)]
    {
        let group = rustix::process::Pid::from_child(child);
        let _ = rustix::process::kill_process_group(group, rustix::process::Signal::KILL);
    }
    let _ = child.kill();
}

fn collect_replies(
    receiver: &mpsc::Receiver<Result<Reply, serde_json::Error>>,
    deadline: Instant,
    limit: Duration,
) -> Collected {
    let mut replies = Vec::new();

    loop {
        let reply = match receiver.recv_timeout(deadline.saturating_duration_since(Instant::now())) {
            Ok(Ok(reply)) => reply,
            Ok(Err(error)) => return Collected::Unreadable(error),
            Err(RecvTimeoutError::Timeout) => {
                tracing::warn!(?limit, "replay worker timed out");
                return Collected::TimedOut;
            }
            Err(RecvTimeoutError::Disconnected) => return Collected::Ended,
        };

        match reply {
            Reply::Started(confinement) => warn_unconfined(&confinement),
            Reply::Failed(failure) => return Collected::Failed(failure),
            Reply::Packet(record) => replies.push(Reply::Packet(record)),
            complete @ (Reply::Header(_) | Reply::Analysis(_) | Reply::Scanned(_)) => {
                replies.push(complete);
                return Collected::Replies(replies);
            }
        }
    }
}

/// Warns once per process when a worker could not confine itself fully.
fn warn_unconfined(confinement: &Confinement) {
    static WARNED: AtomicBool = AtomicBool::new(false);

    if confinement.filesystem == Restriction::Applied && confinement.descriptors == Restriction::Applied {
        return;
    }
    if WARNED.swap(true, Ordering::Relaxed) {
        return;
    }
    tracing::warn!(?confinement, "replay workers run with partial confinement");
}

/// Writes the request and the replay. Stops quietly when the worker stops
/// reading, which it does when it fails early.
fn write_request(stdin: std::process::ChildStdin, request: &Request, replay: &[u8]) {
    let mut writer = BufWriter::new(stdin);
    let header = serde_json::to_vec(request).expect("a request serializes");
    let len = u32::try_from(replay.len()).expect("replays are limited below 4 GiB");

    let _ = writer
        .write_all(&header)
        .and_then(|()| writer.write_all(b"\n"))
        .and_then(|()| writer.write_all(&len.to_le_bytes()))
        .and_then(|()| writer.write_all(replay))
        .and_then(|()| writer.flush());
}

fn read_replies(stdout: std::process::ChildStdout, sender: &mpsc::Sender<Result<Reply, serde_json::Error>>) {
    // An analysis of a large replay stays well below this; it bounds a
    // misbehaving worker.
    const MAX_REPLY_BYTES: u64 = 64 * 1024 * 1024;

    for line in BufReader::new(stdout.take(MAX_REPLY_BYTES)).lines() {
        let Ok(line) = line else {
            return;
        };
        if sender.send(serde_json::from_str(&line)).is_err() {
            return;
        }
    }
}

/// The worker side.
fn serve() -> std::io::Result<()> {
    // Address space for game data, the replay and its decoded packets.
    const ADDRESS_SPACE_BYTES: u64 = 8 * 1024 * 1024 * 1024;
    // Time past the parent's deadline after which a worker exits on its own,
    // in case the parent died and cannot kill it.
    const WORKER_MARGIN: Duration = Duration::from_secs(10);

    let mut input = BufReader::new(std::io::stdin().lock());
    let mut output = BufWriter::new(std::io::stdout().lock());

    let mut line = String::new();
    input.read_line(&mut line)?;
    let request: Request = serde_json::from_str(&line).map_err(std::io::Error::other)?;

    // Ends this worker even if the parent died and cannot kill it.
    let lifetime = request.time_limit() + WORKER_MARGIN;

    // The request comes from the parent and names trusted game data, so it
    // loads before the worker confines itself.
    let job = match Job::prepare(request) {
        Ok(job) => job,
        Err(failure) => return write_reply(&mut output, &Reply::Failed(failure)),
    };

    // Landlock binds this thread and threads it starts, so it is applied
    // before any thread exists; the descriptor limit binds the process. The
    // process limit also forbids threads on Linux, so it comes last.
    crate::sandbox::limit_memory(ADDRESS_SPACE_BYTES);
    let filesystem = Restriction::from_result(crate::sandbox::deny_filesystem());
    let descriptors = Restriction::from_result(crate::sandbox::deny_new_descriptors());
    std::thread::spawn(move || {
        std::thread::sleep(lifetime);
        std::process::exit(1);
    });
    let processes = Restriction::from_result(crate::sandbox::deny_new_processes());
    write_reply(&mut output, &Reply::Started(Confinement { filesystem, descriptors, processes }))?;

    let replay = match read_replay(&mut input)? {
        Ok(replay) => replay,
        Err(failure) => return write_reply(&mut output, &Reply::Failed(failure)),
    };

    // A panic in the parser ends this request, not the worker's reply.
    std::panic::set_hook(Box::new(|_| {}));
    let answered = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| answer(&job, &replay, &mut output)));
    match answered {
        Ok(result) => result,
        Err(_) => write_reply(&mut output, &Reply::Failed(WorkerFailure::Panicked)),
    }
}

fn load_build(source: &BuildSource) -> Result<Loaded, WorkerFailure> {
    let failure = |message: String| WorkerFailure::GameData { dump_dir: source.dump_dir.clone(), message };

    let data = ReplayGameData::load(&source.dump_dir, source.cache_root.as_deref())
        .map_err(|report| failure(report.to_string()))?;
    let results_constants = read_results_constants(&source.dump_dir);
    let constants = GameConstants::for_build(Some(&data.vfs), results_constants.as_ref(), Some(data.version));

    Ok(Loaded { build: LoadedBuild::new(data.provider, constants), results_constants })
}

/// The dump's `constants.json`. `None` when the dump has none, which leaves
/// battle results out of an analysis.
fn read_results_constants(dump_dir: &Path) -> Option<serde_json::Value> {
    let text = std::fs::read_to_string(dump_dir.join("constants.json")).ok()?;

    serde_json::from_str(&text).ok()
}

fn read_replay(input: &mut impl Read) -> std::io::Result<Result<Vec<u8>, WorkerFailure>> {
    let mut len = [0u8; 4];
    input.read_exact(&mut len)?;
    let bytes = ByteCount::new(u32::from_le_bytes(len) as usize);
    if bytes > MAX_REPLAY_BYTES {
        return Ok(Err(WorkerFailure::TooLarge { bytes, limit: MAX_REPLAY_BYTES }));
    }

    let mut replay = vec![0; bytes.get()];
    input.read_exact(&mut replay)?;

    Ok(Ok(replay))
}

fn answer(job: &Job, replay: &[u8], output: &mut impl Write) -> std::io::Result<()> {
    let reply = match job {
        Job::Header => match ReplayFile::meta_from_bytes(replay) {
            Ok(meta) => Reply::Header(ReplayHeader {
                version: Version::try_from_client_exe(&meta.clientVersionFromExe),
                client_version: meta.clientVersionFromExe,
                player: meta.playerName,
                player_ship: meta.playerVehicle,
                map: meta.mapName,
                game_type: meta.gameType,
                date_time: meta.dateTime,
            }),
            Err(report) => Reply::Failed(WorkerFailure::Replay { message: report.current_context().to_string() }),
        },
        Job::Analyze(loaded) => match ReplayFile::from_bytes(replay) {
            Ok(replay) => match replayshark::analyze(&replay, &loaded.build, loaded.results_constants.as_ref()) {
                Ok(analysis) => Reply::Analysis(Box::new(analysis)),
                Err(error) => Reply::Failed(WorkerFailure::Analysis { message: error.to_string() }),
            },
            Err(report) => Reply::Failed(WorkerFailure::Replay { message: report.current_context().to_string() }),
        },
        Job::Packets { loaded, filter, limits } => match ReplayFile::from_bytes(replay) {
            Ok(replay) => return write_packets(&replay, &loaded.build, filter, *limits, output),
            Err(report) => Reply::Failed(WorkerFailure::Replay { message: report.current_context().to_string() }),
        },
    };

    write_reply(output, &reply)
}

/// Streams admitted packets while within `limits`, then the scan's totals.
fn write_packets(
    replay: &ReplayFile,
    build: &LoadedBuild,
    filter: &PacketFilter,
    limits: PacketLimits,
    output: &mut impl Write,
) -> std::io::Result<()> {
    let max_records = limits.records.get();
    let max_bytes = limits.bytes.get();
    let mut sent = 0;
    let mut sent_bytes = 0;
    let mut write_error = None;
    let scanned = replayshark::scan_packets(replay, build, filter, |record| {
        if sent >= max_records || sent_bytes >= max_bytes {
            // Later matches are still counted.
            return ControlFlow::Continue(());
        }

        let reply = Reply::Packet(record);
        let line = match serde_json::to_vec(&reply) {
            Ok(line) => line,
            Err(error) => {
                write_error = Some(std::io::Error::other(error));
                return ControlFlow::Break(());
            }
        };
        if sent_bytes + line.len() > max_bytes {
            sent_bytes = max_bytes;
            return ControlFlow::Continue(());
        }
        if let Err(error) = output.write_all(&line).and_then(|()| output.write_all(b"\n")) {
            write_error = Some(error);
            return ControlFlow::Break(());
        }
        sent += 1;
        sent_bytes += line.len();

        ControlFlow::Continue(())
    });
    if let Some(error) = write_error {
        return Err(error);
    }

    let reply = match scanned {
        Ok(scan) => Reply::Scanned(scan),
        Err(error) => Reply::Failed(WorkerFailure::Analysis { message: error.to_string() }),
    };

    write_reply(output, &reply)
}

fn write_reply(output: &mut impl Write, reply: &Reply) -> std::io::Result<()> {
    serde_json::to_writer(&mut *output, reply).map_err(std::io::Error::other)?;
    output.write_all(b"\n")?;

    output.flush()
}

#[cfg(all(test, target_os = "linux"))]
mod tests {
    use std::io::ErrorKind;
    use std::net::TcpListener;
    use std::net::TcpStream;
    use std::net::UdpSocket;
    use std::path::PathBuf;
    use std::process::Command;
    use std::time::Duration;

    /// Names a directory holding a sentinel file the probe tries to read.
    const PROBE_ENV: &str = "WOWS_QUERY_REPLAY_PROBE";
    /// Names a local port with a TCP listener and a UDP socket bound to it.
    const PORT_ENV: &str = "WOWS_QUERY_REPLAY_PROBE_PORT";

    fn kind(error: std::io::Error) -> String {
        match error.raw_os_error() {
            // EMFILE has no stable ErrorKind.
            Some(24) => "EMFILE".to_owned(),
            _ => format!("{:?}", error.kind()),
        }
    }

    fn outcome(result: std::io::Result<()>) -> String {
        result.map_or_else(kind, |()| "Ok".to_owned())
    }

    /// Run in a child process by `confined_replay_worker_cannot_escape`, since
    /// the confinement cannot be lifted from a process.
    #[test]
    fn replay_probe() {
        let Some(dir) = std::env::var_os(PROBE_ENV).map(PathBuf::from) else {
            return;
        };
        let port: u16 = std::env::var(PORT_ENV).expect("probe port").parse().expect("numeric probe port");

        let filesystem = crate::sandbox::deny_filesystem();
        let descriptors = crate::sandbox::deny_new_descriptors();
        let read = outcome(std::fs::read(dir.join("sentinel")).map(drop));
        let listed = outcome(std::fs::read_dir(&dir).map(drop));
        let created = outcome(std::fs::write(dir.join("created"), b""));
        let root = outcome(std::fs::read_dir("/").map(drop));
        let tcp = outcome(TcpStream::connect(("127.0.0.1", port)).map(drop));
        let udp = outcome(
            UdpSocket::bind("127.0.0.1:0").and_then(|socket| socket.send_to(b"probe", ("127.0.0.1", port))).map(drop),
        );
        println!(
            "PROBE filesystem={filesystem:?} descriptors={descriptors:?} read={read} list={listed} create={created} \
             root={root} tcp={tcp} udp={udp}"
        );
    }

    #[test]
    fn confined_replay_worker_cannot_escape() {
        let dir = std::env::temp_dir().join(format!("wows-query-replay-probe-{}", std::process::id()));
        std::fs::create_dir_all(&dir).expect("create the probe directory");
        std::fs::write(dir.join("sentinel"), b"sentinel").expect("write the sentinel file");
        let listener = TcpListener::bind("127.0.0.1:0").expect("bind the TCP listener");
        let port = listener.local_addr().expect("listener address").port();
        let udp = UdpSocket::bind(("127.0.0.1", port)).expect("bind the UDP socket on the same port");
        listener.set_nonblocking(true).expect("nonblocking listener");
        udp.set_read_timeout(Some(Duration::from_millis(100))).expect("UDP read timeout");

        let output = Command::new(std::env::current_exe().expect("this test's executable"))
            .args(["--exact", "replay_worker::tests::replay_probe", "--nocapture", "--test-threads=1"])
            .env(PROBE_ENV, &dir)
            .env(PORT_ENV, port.to_string())
            .output()
            .expect("run the probe");
        let created = dir.join("created").exists();
        let _ = std::fs::remove_dir_all(&dir);
        let connected = listener.accept().map_err(|error| error.kind());
        let mut buffer = [0u8; 16];
        let received = udp.recv_from(&mut buffer).map_err(|error| error.kind());
        let stdout = String::from_utf8_lossy(&output.stdout);
        let line = stdout
            .lines()
            .find_map(|line| line.split_once("PROBE ").map(|(_, probe)| probe))
            .unwrap_or_else(|| panic!("the probe printed nothing: {stdout}"));
        println!("{line}");

        assert!(!created, "the probe created a file: {line}");
        assert!(matches!(connected, Err(ErrorKind::WouldBlock)), "the probe connected over TCP: {line}");
        assert!(
            matches!(received, Err(ErrorKind::WouldBlock | ErrorKind::TimedOut)),
            "the probe sent a UDP datagram: {line}"
        );
        // The descriptor limit rejects every open before Landlock is consulted;
        // jq_worker's probe covers Landlock alone.
        assert_eq!(
            line,
            "filesystem=Ok(()) descriptors=Ok(()) read=EMFILE list=EMFILE create=EMFILE root=EMFILE tcp=EMFILE udp=EMFILE"
        );
    }
}

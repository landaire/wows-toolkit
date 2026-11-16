//! Runs jq filters in a child process.
//!
//! jaq cannot be interrupted, and a filter can recurse until the stack
//! overflows, which aborts the process, or allocate without bound. The
//! filter therefore runs in a worker process that is killed at a deadline or
//! when it holds more memory than [`Limits::memory_bytes`], and, on Linux,
//! limited in address space and denied filesystem access with Landlock. The
//! worker is the host program itself, started again with [`WORKER_ENV`] set:
//! call [`init`] first in `main` of every program that serves
//! `gameparams_query`.
//!
//! A request names its inputs in one of two ways. A store request names
//! entries of the raw GameParams store named by [`STORE_ENV`], which the
//! worker maps before it denies itself filesystem access, so filters read
//! entries in place from pages every process shares. A JSON request is
//! followed by each input as a 4-byte little-endian length and its JSON
//! text, until the parent closes the pipe. The parent writes one JSON
//! request line; the worker answers with one JSON [`Reply`] line per input
//! and a final line.

use std::io::BufRead;
use std::io::BufReader;
use std::io::BufWriter;
use std::io::Read;
use std::io::Write;
use std::path::Path;
use std::path::PathBuf;
use std::process::ChildStdin;
use std::process::ChildStdout;
use std::process::Command;
use std::process::Stdio;
use std::sync::OnceLock;
use std::sync::mpsc;
use std::sync::mpsc::RecvTimeoutError;
use std::time::Duration;
use std::time::Instant;

use serde::Deserialize;
use serde::Serialize;

use crate::error::CompileError;
use crate::error::JqError;
use crate::error::RawParamsError;
use crate::error::StoreDamage;
use crate::jq::Program;
use crate::jq_input::Inputs;
use crate::raw_params::EntryPosition;
use crate::raw_params::RawParams;
use crate::sandbox::WorkerSlots;

pub use crate::jq::InputOutputs;
pub use crate::jq::Limits;
pub use crate::jq::Stop;

/// Set in the environment of a worker process.
pub const WORKER_ENV: &str = "WOWS_QUERY_JQ_WORKER";

/// Set in the environment of a worker process to the store it reads.
pub const STORE_ENV: &str = "WOWS_QUERY_JQ_STORE";

static WORKER_PROGRAM: OnceLock<PathBuf> = OnceLock::new();

/// Time past the filter's limit the parent waits before killing a worker.
const GRACE: Duration = Duration::from_secs(2);

/// Time past the filter's limit after which a worker exits on its own.
/// Larger than [`GRACE`], so the parent always times out first.
const WORKER_MARGIN: Duration = Duration::from_secs(10);

const _: () = assert!(WORKER_MARGIN.as_millis() > GRACE.as_millis(), "the parent must time out first");

/// Distinguishes the working directories of concurrent workers.
static NEXT_WORKDIR: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);

/// Each worker may use up to its address-space limit, so further callers
/// wait.
static WORKER_SLOTS: WorkerSlots = WorkerSlots::new(2);

/// Outputs of a filter over a sequence of inputs.
#[derive(Debug)]
pub struct Evaluation {
    /// One entry per input evaluated, in input order.
    pub inputs: Vec<InputOutputs>,
    /// Size of every output value as compact JSON.
    pub output_bytes: usize,
    /// Why evaluation ended before every input was evaluated in full.
    pub stopped: Option<Stop>,
}

#[derive(Serialize, Deserialize)]
struct Request {
    program: String,
    limits: Limits,
    inputs: RequestInputs,
}

/// Where the worker finds the inputs of a request.
#[derive(Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
enum RequestInputs {
    /// Entries of the store named by [`STORE_ENV`].
    Store(Vec<EntryPosition>),
    /// Length-prefixed JSON texts after the request line.
    Json,
}

/// Why a worker could not read the store.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, thiserror::Error)]
#[serde(rename_all = "snake_case")]
pub enum StoreFailure {
    #[error("the worker could not open or map the store")]
    Unreadable,

    #[error("the store is damaged: {0}")]
    Damaged(StoreDamage),

    #[error("entry {} of the store is damaged: {damage}", position.index())]
    DamagedEntry { position: EntryPosition, damage: StoreDamage },
}

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
enum Reply {
    /// The worker read the request; sent before anything the filter does.
    Started(Confinement),
    Compile(CompileError),
    Store(StoreFailure),
    Input(InputOutputs),
    /// Evaluation ended early; no further replies follow.
    Stopped(Stop),
    /// Every input was evaluated.
    Done,
}

/// Whether the worker could deny itself filesystem access.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
enum Confinement {
    /// No file can be opened, listed or created.
    FilesystemDenied,
    Unavailable {
        reason: String,
    },
}

/// Whether the worker sent [`Reply::Started`] before its replies ended.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Progress {
    NotStarted,
    Started,
}

/// What the parent hears about a running worker.
enum Event {
    Reply(Result<Reply, serde_json::Error>),
    /// The worker's output ended.
    Ended,
    /// The worker holds more memory than its limit.
    OverMemory,
}

/// In a worker process, evaluates the request on standard input and exits.
/// Otherwise registers this program as the jq worker. Call it first in
/// `main`, before anything that reads standard input.
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
        Err(error) => tracing::warn!(%error, "cannot locate this program; jq filters are unavailable"),
    }
}

/// Uses `program` as the jq worker: a program whose `main` calls [`init`]
/// first, such as the `wows-query-jq-worker` binary. The first call wins.
pub fn set_worker_program(program: PathBuf) {
    // The worker starts in its own working directory, where a relative path
    // names nothing. Only an empty path fails, and it names nothing either.
    let program = std::path::absolute(&program).unwrap_or(program);
    // A later call names the same kind of worker; keeping the first is correct.
    let _ = WORKER_PROGRAM.set(program);
}

/// Runs `program` once per entry of `store` in a worker process, which
/// reads the entries from the store file itself. Two workers run at once; a
/// caller waits at most 15 s for one.
pub fn evaluate(
    program: &str,
    store: &RawParams,
    entries: &[EntryPosition],
    limits: Limits,
) -> Result<Evaluation, JqError> {
    // The worker starts in its own working directory, where a relative path
    // names nothing. Only an empty path fails, and a store has a file name.
    let store_path = std::path::absolute(store.path()).unwrap_or_else(|_| store.path().to_path_buf());
    let request = Request { program: program.to_owned(), limits, inputs: RequestInputs::Store(entries.to_vec()) };

    run_worker(&request, Some(&store_path), std::iter::empty())
}

/// Runs `program` once per input, each a JSON text, in a worker process.
/// Inputs are pulled as the worker consumes them, so only a few are held at
/// a time. Two workers run at once; a caller waits at most 15 s for one.
pub fn evaluate_json(
    program: &str,
    inputs: impl Iterator<Item = Vec<u8>> + Send,
    limits: Limits,
) -> Result<Evaluation, JqError> {
    let request = Request { program: program.to_owned(), limits, inputs: RequestInputs::Json };

    run_worker(&request, None, inputs)
}

/// Starts a worker that maps `store`, if given, writes `request` and then
/// `records` to it, and collects its replies.
fn run_worker(
    request: &Request,
    store: Option<&Path>,
    records: impl Iterator<Item = Vec<u8>> + Send,
) -> Result<Evaluation, JqError> {
    let Some(worker) = WORKER_PROGRAM.get() else {
        return Err(JqError::NoWorker);
    };
    // Each `gameparams_query` worker runs at most 5 s plus 2 s of grace;
    // this lets two evaluations ahead of the caller finish.
    const MAX_WAIT: Duration = Duration::from_secs(15);
    let Some(_permit) = WORKER_SLOTS.acquire(MAX_WAIT) else {
        return Err(JqError::Busy { waited: MAX_WAIT });
    };
    // An empty working directory, so nothing the host keeps there is reachable.
    let workdir = std::env::temp_dir().join(format!(
        "wows-query-jq-{}-{}",
        std::process::id(),
        NEXT_WORKDIR.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
    ));
    std::fs::create_dir_all(&workdir).map_err(|source| JqError::Workdir { path: workdir.clone(), source })?;
    let mut command = Command::new(worker);
    // The filter must not see the host's secrets; `env` is also disabled.
    command.env_clear().env(WORKER_ENV, "1").current_dir(&workdir);
    if let Some(store) = store {
        command.env(STORE_ENV, store);
    }
    // Windows programs fail to start some system services without it.
    #[cfg(windows)]
    if let Some(system_root) = std::env::var_os("SystemRoot") {
        command.env("SystemRoot", system_root);
    }
    let spawned = command.stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(Stdio::null()).spawn();
    let mut child = match spawned {
        Ok(child) => child,
        Err(source) => {
            let _ = std::fs::remove_dir(&workdir);
            return Err(JqError::Spawn { program: worker.clone(), source });
        }
    };
    let stdin = child.stdin.take().expect("stdin is piped");
    let stdout = child.stdout.take().expect("stdout is piped");

    let limits = request.limits;
    let deadline = Instant::now() + limits.time + GRACE;
    let mut evaluation = Evaluation { inputs: Vec::new(), output_bytes: 0, stopped: None };
    let pid = child.id();
    let outcome = std::thread::scope(|scope| {
        let (sender, receiver) = mpsc::channel();
        let (watching, stop_watching) = mpsc::channel::<()>();
        let watch_sender = sender.clone();
        let writer = scope.spawn(move || write_request(stdin, request, records));
        scope.spawn(move || read_replies(stdout, &sender));
        scope.spawn(move || watch_memory(pid, limits.memory_bytes, &stop_watching, &watch_sender));

        let outcome = collect_replies(&receiver, deadline, &mut evaluation);
        drop(watching);
        // Ends a worker past its deadline or its memory limit, and with it both pipe threads.
        let _ = child.kill();

        let written = writer.join().unwrap_or_else(|panic| std::panic::resume_unwind(panic));
        written.and(outcome)
    });
    let status = child.wait().ok();
    // The directory stays empty; a failure leaves an empty directory behind.
    let _ = std::fs::remove_dir(&workdir);

    // A worker that ends before it starts is misconfigured, not a filter
    // that crashed it.
    if outcome? == Progress::NotStarted && evaluation.stopped == Some(Stop::Crashed) {
        return Err(JqError::Exited { status });
    }

    Ok(evaluation)
}

fn collect_replies(
    receiver: &mpsc::Receiver<Event>,
    deadline: Instant,
    evaluation: &mut Evaluation,
) -> Result<Progress, JqError> {
    let mut progress = Progress::NotStarted;

    loop {
        let reply = match receiver.recv_timeout(deadline.saturating_duration_since(Instant::now())) {
            Ok(Event::Reply(reply)) => reply.map_err(JqError::Reply)?,
            Err(RecvTimeoutError::Timeout) => {
                evaluation.stopped = Some(Stop::TimeLimit);
                return Ok(progress);
            }
            // The memory limit kills the worker as an address-space limit would have crashed it.
            Ok(Event::Ended | Event::OverMemory) | Err(RecvTimeoutError::Disconnected) => {
                evaluation.stopped = Some(Stop::Crashed);
                return Ok(progress);
            }
        };

        match reply {
            Reply::Started(confinement) => {
                warn_unconfined(confinement);
                progress = Progress::Started;
            }
            Reply::Compile(error) => return Err(JqError::Compile(error)),
            Reply::Store(failure) => return Err(JqError::Store(failure)),
            Reply::Input(outputs) => {
                evaluation.output_bytes += outputs.output_bytes;
                evaluation.inputs.push(outputs);
            }
            Reply::Stopped(stop) => {
                evaluation.stopped = Some(stop);
                return Ok(progress);
            }
            Reply::Done => return Ok(progress),
        }
    }
}

/// Warns once per process when workers run with filesystem access.
fn warn_unconfined(confinement: Confinement) {
    static WARNED: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

    let Confinement::Unavailable { reason } = confinement else {
        return;
    };
    if WARNED.swap(true, std::sync::atomic::Ordering::Relaxed) {
        return;
    }
    tracing::warn!(%reason, "jq workers run with filesystem access");
}

/// Writes the request, then each record as its 4-byte little-endian length
/// and its text. Stops quietly when the worker stops reading, which it does
/// once evaluation ends early; its replies, or their absence, say why.
fn write_request(stdin: ChildStdin, request: &Request, records: impl Iterator<Item = Vec<u8>>) -> Result<(), JqError> {
    let mut writer = BufWriter::new(stdin);
    let mut header = serde_json::to_vec(request).expect("a request serializes");
    header.push(b'\n');
    if writer.write_all(&header).is_err() {
        return Ok(());
    }

    for record in records {
        let len = u32::try_from(record.len()).map_err(|_| JqError::InputTooLarge { bytes: record.len() })?;
        if writer.write_all(&len.to_le_bytes()).and_then(|()| writer.write_all(&record)).is_err() {
            return Ok(());
        }
    }

    // A failed flush means the worker already stopped reading.
    let _ = writer.flush();

    Ok(())
}

fn read_replies(stdout: ChildStdout, sender: &mpsc::Sender<Event>) {
    // Replies stay near the output limit; this bounds a misbehaving worker.
    const MAX_REPLY_BYTES: u64 = 16 * 1024 * 1024;

    for line in BufReader::new(stdout.take(MAX_REPLY_BYTES)).lines() {
        let Ok(line) = line else {
            break;
        };
        if sender.send(Event::Reply(serde_json::from_str(&line))).is_err() {
            return;
        }
    }
    // The parent has stopped listening when this fails.
    let _ = sender.send(Event::Ended);
}

/// Reports when worker `pid` holds more than `limit` bytes, until `stop` ends.
/// The address-space limit covers this on Linux, but not on Windows, where
/// a worker's commit can starve every process, or on macOS.
fn watch_memory(pid: u32, limit: u64, stop: &mpsc::Receiver<()>, sender: &mpsc::Sender<Event>) {
    // A worker writing gigabytes a second is stopped near the limit.
    const POLL: Duration = Duration::from_millis(20);

    loop {
        if crate::sandbox::process_memory(pid).is_some_and(|held| held > limit) {
            // The parent has stopped listening when this fails.
            let _ = sender.send(Event::OverMemory);
            return;
        }
        match stop.recv_timeout(POLL) {
            Err(RecvTimeoutError::Timeout) => {}
            Ok(()) | Err(RecvTimeoutError::Disconnected) => return,
        }
    }
}

/// The worker side: maps the store, if one is named, denies itself
/// filesystem access, then evaluates on a thread with a large stack, so
/// deeply nested values a filter builds do not overflow the main thread's
/// stack.
fn serve() -> std::io::Result<()> {
    // Reserved address space; pages are committed as used.
    const STACK_BYTES: usize = 256 * 1024 * 1024;

    // Filters get 4 GiB of address space.
    crate::sandbox::limit_memory(4 * 1024 * 1024 * 1024);
    let store = match std::env::var_os(STORE_ENV) {
        Some(path) => RawParams::open_file(Path::new(&path)).map_err(|error| match error {
            RawParamsError::Damaged { damage, .. } => StoreFailure::Damaged(damage),
            _ => StoreFailure::Unreadable,
        }),
        None => Err(StoreFailure::Unreadable),
    };
    let confinement = deny_filesystem();
    std::thread::scope(|scope| {
        let worker = std::thread::Builder::new()
            .name("jq".to_owned())
            .stack_size(STACK_BYTES)
            .spawn_scoped(scope, move || {
                serve_request(std::io::stdin().lock(), std::io::stdout().lock(), confinement, store)
            })?;

        worker.join().unwrap_or_else(|panic| std::panic::resume_unwind(panic))
    })
}

fn serve_request(
    mut input: impl BufRead,
    mut output: impl Write,
    confinement: Confinement,
    store: Result<RawParams, StoreFailure>,
) -> std::io::Result<()> {
    let mut header = String::new();
    input.read_line(&mut header)?;
    let request: Request = serde_json::from_str(&header).map_err(std::io::Error::other)?;
    write_reply(&mut output, &Reply::Started(confinement))?;

    // Ends this worker even if the parent died and cannot kill it; filters
    // that iterate over data spend no steps and never see the deadline.
    // The parent's deadline comes first, so it reports `time_limit` rather
    // than a crash.
    let lifetime = request.limits.time + WORKER_MARGIN;
    std::thread::spawn(move || {
        std::thread::sleep(lifetime);
        std::process::exit(1);
    });

    let program = match Program::compile(&request.program) {
        Ok(program) => program,
        Err(error) => return write_reply(&mut output, &Reply::Compile(error)),
    };

    let limits = request.limits;
    let deadline = Instant::now() + limits.time;
    let mut output_left = limits.output_bytes;
    match request.inputs {
        RequestInputs::Store(entries) => {
            let store = match store {
                Ok(store) => store,
                Err(failure) => return write_reply(&mut output, &Reply::Store(failure)),
            };
            let mut inputs = Inputs::new(store.tables());
            for position in entries {
                let Some(entry) = store.entry(position) else {
                    return write_reply(&mut output, &Reply::Store(StoreFailure::Damaged(StoreDamage::Entry)));
                };
                let value = match inputs.value(entry.root()) {
                    Ok(value) => value,
                    Err(damage) => {
                        return write_reply(
                            &mut output,
                            &Reply::Store(StoreFailure::DamagedEntry { position, damage }),
                        );
                    }
                };
                let (outputs, stop) = program.run(value, limits.steps_per_input, deadline, &mut output_left);
                if let Some(stop) = reply_input(&mut output, outputs, stop, deadline)? {
                    return write_reply(&mut output, &Reply::Stopped(stop));
                }
            }
        }
        RequestInputs::Json => {
            let mut record = Vec::new();
            while read_record(&mut input, &mut record)? {
                let (outputs, stop) = program.run_json(&record, limits.steps_per_input, deadline, &mut output_left);
                if let Some(stop) = reply_input(&mut output, outputs, stop, deadline)? {
                    return write_reply(&mut output, &Reply::Stopped(stop));
                }
            }
        }
    }

    write_reply(&mut output, &Reply::Done)
}

/// Replies with the outputs of one input. Returns why evaluation ends here:
/// the input's own stop, or the deadline passing during it.
fn reply_input(
    output: &mut impl Write,
    outputs: InputOutputs,
    stop: Option<Stop>,
    deadline: Instant,
) -> std::io::Result<Option<Stop>> {
    write_reply(output, &Reply::Input(outputs))?;

    Ok(stop.or_else(|| (Instant::now() >= deadline).then_some(Stop::TimeLimit)))
}

/// Reads one length-prefixed record into `record`; `false` at the end of
/// input.
fn read_record(input: &mut impl Read, record: &mut Vec<u8>) -> std::io::Result<bool> {
    let mut len = [0u8; 4];
    match input.read_exact(&mut len) {
        Ok(()) => {}
        Err(error) if error.kind() == std::io::ErrorKind::UnexpectedEof => return Ok(false),
        Err(error) => return Err(error),
    }

    record.clear();
    record.resize(u32::from_le_bytes(len) as usize, 0);
    input.read_exact(record)?;

    Ok(true)
}

fn write_reply(output: &mut impl Write, reply: &Reply) -> std::io::Result<()> {
    serde_json::to_writer(&mut *output, reply).map_err(std::io::Error::other)?;
    output.write_all(b"\n")?;

    output.flush()
}

/// Denies the worker filesystem access; see [`crate::sandbox::deny_filesystem`].
fn deny_filesystem() -> Confinement {
    match crate::sandbox::deny_filesystem() {
        Ok(()) => Confinement::FilesystemDenied,
        Err(error) => Confinement::Unavailable { reason: error.reason() },
    }
}

#[cfg(all(test, target_os = "linux"))]
mod tests {
    use super::*;

    /// Names a file the probe tries to read, and a directory it tries to
    /// list and write in.
    const PROBE_ENV: &str = "WOWS_QUERY_LANDLOCK_PROBE";

    /// Run in a child process by `confined_worker_cannot_use_files`, since
    /// Landlock cannot be lifted from a process.
    #[test]
    fn landlock_probe() {
        let Some(dir) = std::env::var_os(PROBE_ENV).map(PathBuf::from) else {
            return;
        };

        let confinement = deny_filesystem();
        let read = std::fs::read(dir.join("sentinel")).map(drop).map_err(|error| error.kind());
        let listed = std::fs::read_dir(&dir).map(drop).map_err(|error| error.kind());
        let created = std::fs::write(dir.join("created"), b"").map_err(|error| error.kind());
        let root = std::fs::read_dir("/").map(drop).map_err(|error| error.kind());
        println!("PROBE {confinement:?} read={read:?} list={listed:?} create={created:?} root={root:?}");
    }

    #[test]
    fn confined_worker_cannot_use_files() {
        let dir = std::env::temp_dir().join(format!("wows-query-landlock-{}", std::process::id()));
        std::fs::create_dir_all(&dir).expect("create the probe directory");
        std::fs::write(dir.join("sentinel"), b"sentinel").expect("write the sentinel file");

        let output = Command::new(std::env::current_exe().expect("this test's executable"))
            .args(["--exact", "jq_worker::tests::landlock_probe", "--nocapture", "--test-threads=1"])
            .env(PROBE_ENV, &dir)
            .output()
            .expect("run the probe");
        let created = dir.join("created").exists();
        let _ = std::fs::remove_dir_all(&dir);
        let stdout = String::from_utf8_lossy(&output.stdout);
        let line = stdout
            .lines()
            .find_map(|line| line.split_once("PROBE ").map(|(_, probe)| probe))
            .unwrap_or_else(|| panic!("the probe printed nothing: {stdout}"));
        println!("{line}");

        assert!(!created, "the probe created a file: {line}");
        assert_eq!(
            line,
            "FilesystemDenied read=Err(PermissionDenied) list=Err(PermissionDenied) create=Err(PermissionDenied) \
             root=Err(PermissionDenied)"
        );
    }
}

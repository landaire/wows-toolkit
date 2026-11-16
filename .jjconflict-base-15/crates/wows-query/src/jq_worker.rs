//! Runs jq filters in a child process.
//!
//! jaq cannot be interrupted, and a filter can recurse until the stack
//! overflows, which aborts the process, or allocate without bound. The
//! filter therefore runs in a worker process that is killed at a deadline
//! and, on Linux, limited in address space and denied filesystem access
//! with Landlock. The worker is the host program itself, started again with
//! [`WORKER_ENV`] set: call [`init`] first in `main` of every program that
//! serves `gameparams_query`.
//!
//! The parent writes one JSON request line, then each input as a 4-byte
//! little-endian length and its JSON text, then closes the pipe. The worker
//! answers with one JSON [`Reply`] line per input and a final line.

use std::io::BufRead;
use std::io::BufReader;
use std::io::BufWriter;
use std::io::Read;
use std::io::Write;
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
use crate::jq::Program;
use crate::sandbox::WorkerSlots;

pub use crate::jq::InputOutputs;
pub use crate::jq::Limits;
pub use crate::jq::Stop;

/// Set in the environment of a worker process.
pub const WORKER_ENV: &str = "WOWS_QUERY_JQ_WORKER";

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
}

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
enum Reply {
    /// The worker read the request; sent before anything the filter does.
    Started(Confinement),
    Compile(CompileError),
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

/// Runs `program` once per input, each a JSON text, in a worker process.
/// Inputs are pulled as the worker consumes them, so only a few are held at
/// a time. Two workers run at once; a caller waits at most 15 s for one.
pub fn evaluate(
    program: &str,
    inputs: impl Iterator<Item = Vec<u8>> + Send,
    limits: Limits,
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
    let request = Request { program: program.to_owned(), limits };

    let deadline = Instant::now() + limits.time + GRACE;
    let mut evaluation = Evaluation { inputs: Vec::new(), output_bytes: 0, stopped: None };
    let outcome = std::thread::scope(|scope| {
        let (sender, receiver) = mpsc::channel();
        let writer = scope.spawn(move || write_requests(stdin, &request, inputs));
        scope.spawn(move || read_replies(stdout, &sender));

        let outcome = collect_replies(&receiver, deadline, &mut evaluation);
        // Ends a worker past its deadline, and with it both pipe threads.
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
    receiver: &mpsc::Receiver<Result<Reply, serde_json::Error>>,
    deadline: Instant,
    evaluation: &mut Evaluation,
) -> Result<Progress, JqError> {
    let mut progress = Progress::NotStarted;

    loop {
        let reply = match receiver.recv_timeout(deadline.saturating_duration_since(Instant::now())) {
            Ok(reply) => reply.map_err(JqError::Reply)?,
            Err(RecvTimeoutError::Timeout) => {
                evaluation.stopped = Some(Stop::TimeLimit);
                return Ok(progress);
            }
            Err(RecvTimeoutError::Disconnected) => {
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

/// Writes the request and inputs. Stops quietly when the worker stops
/// reading, which it does once evaluation ends early.
fn write_requests(stdin: ChildStdin, request: &Request, inputs: impl Iterator<Item = Vec<u8>>) -> Result<(), JqError> {
    let mut writer = BufWriter::new(stdin);
    let header = serde_json::to_vec(request).expect("a request serializes");
    if writer.write_all(&header).and_then(|()| writer.write_all(b"\n")).is_err() {
        return Ok(());
    }

    for input in inputs {
        let len = u32::try_from(input.len()).map_err(|_| JqError::InputTooLarge { bytes: input.len() })?;
        if writer.write_all(&len.to_le_bytes()).and_then(|()| writer.write_all(&input)).is_err() {
            return Ok(());
        }
    }

    // A failed flush means the worker already stopped reading.
    let _ = writer.flush();

    Ok(())
}

fn read_replies(stdout: ChildStdout, sender: &mpsc::Sender<Result<Reply, serde_json::Error>>) {
    // Replies stay near the output limit; this bounds a misbehaving worker.
    const MAX_REPLY_BYTES: u64 = 16 * 1024 * 1024;

    for line in BufReader::new(stdout.take(MAX_REPLY_BYTES)).lines() {
        let Ok(line) = line else {
            return;
        };
        if sender.send(serde_json::from_str(&line)).is_err() {
            return;
        }
    }
}

/// The worker side: denies itself filesystem access, then evaluates on a
/// thread with a large stack, so deeply nested values a filter builds do not
/// overflow the main thread's stack.
fn serve() -> std::io::Result<()> {
    // Reserved address space; pages are committed as used.
    const STACK_BYTES: usize = 256 * 1024 * 1024;

    // Filters get 4 GiB of address space.
    crate::sandbox::limit_memory(4 * 1024 * 1024 * 1024);
    let confinement = deny_filesystem();
    std::thread::scope(|scope| {
        let worker = std::thread::Builder::new()
            .name("jq".to_owned())
            .stack_size(STACK_BYTES)
            .spawn_scoped(scope, move || {
                serve_request(std::io::stdin().lock(), std::io::stdout().lock(), confinement)
            })?;

        worker.join().unwrap_or_else(|panic| std::panic::resume_unwind(panic))
    })
}

fn serve_request(mut input: impl BufRead, mut output: impl Write, confinement: Confinement) -> std::io::Result<()> {
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
    let mut record = Vec::new();
    while read_record(&mut input, &mut record)? {
        let (outputs, stop) = program.run(&record, limits.steps_per_input, deadline, &mut output_left);
        write_reply(&mut output, &Reply::Input(outputs))?;

        let stop = stop.or_else(|| (Instant::now() >= deadline).then_some(Stop::TimeLimit));
        if let Some(stop) = stop {
            return write_reply(&mut output, &Reply::Stopped(stop));
        }
    }

    write_reply(&mut output, &Reply::Done)
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

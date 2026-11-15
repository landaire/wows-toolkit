//! Every thread of the client and wineserver, read from `/proc` without stopping it. Wine's async
//! socket I/O waits in wineserver, so its threads matter as much as the client's. The `syscall` row
//! ends with the user stack pointer and program counter, which `maps.txt` places in a module.
//! Ptrace-based unwinders are not used: they stop the process, and stopping wineserver stalls every
//! Wine thread, which disturbs the network calls under observation.

use std::fmt::Write as _;
use std::io::ErrorKind;
use std::time::Duration;
use std::time::Instant;

use crate::diagnostics::Clock;
use crate::diagnostics::StopSignal;
use crate::diagnostics::Target;
use crate::diagnostics::output::CaptureWriter;
use crate::diagnostics::procfs;
use crate::error::CaptureError;

/// Samples each of `targets` every `INTERVAL` until stopped.
pub(super) fn sample(
    targets: &[Target],
    mut out: CaptureWriter,
    clock: &Clock,
    stop: &StopSignal,
) -> Result<(), CaptureError> {
    const INTERVAL: Duration = Duration::from_millis(1000);

    // The kernel stack needs CAP_SYS_ADMIN; without it, the first denial is noted and later reads
    // are skipped.
    let mut kernel_stacks = true;
    loop {
        let round = Instant::now();
        let mut text = String::new();
        for target in targets {
            let _ = writeln!(text, "== t={} pid={} {}", clock.now(), target.pid.0, target.label);
            let threads = match procfs::threads(target.pid) {
                Ok(threads) => threads,
                Err(err) => {
                    let _ = writeln!(text, "error: {err}");
                    continue;
                }
            };

            for tid in threads {
                let task = format!("/proc/{}/task/{tid}", target.pid.0);
                // Threads exit between listing and reading.
                let Ok(stat) = std::fs::read_to_string(format!("{task}/stat")) else {
                    continue;
                };
                let Some(stat) = procfs::ThreadStat::parse(&stat) else {
                    continue;
                };
                let syscall = read_line(&format!("{task}/syscall"));
                let wchan = read_line(&format!("{task}/wchan"));
                let _ = writeln!(
                    text,
                    "{tid} {} {} cpu={} wchan={wchan} syscall={syscall}",
                    stat.comm, stat.state, stat.cpu
                );

                if kernel_stacks {
                    match std::fs::read_to_string(format!("{task}/stack")) {
                        Ok(stack) => text.push_str(&stack),
                        Err(err) if err.kind() == ErrorKind::PermissionDenied => {
                            kernel_stacks = false;
                            let _ = writeln!(text, "kernel stacks unavailable: {err}");
                        }
                        Err(_) => {}
                    }
                }
            }
        }
        out.record(text.as_bytes())?;

        if stop.wait(INTERVAL.saturating_sub(round.elapsed())) {
            return Ok(());
        }
    }
}

fn read_line(path: &str) -> String {
    match std::fs::read_to_string(path) {
        Ok(text) => text.trim_end().to_owned(),
        Err(err) => format!("error: {err}"),
    }
}

//! Limits a worker process places on itself before it reads untrusted input.

use std::sync::Condvar;
use std::sync::Mutex;
use std::sync::PoisonError;
use std::time::Duration;
use std::time::Instant;

/// Workers of one kind that may run at once. Each may use up to its
/// address-space limit, so further callers wait.
pub(crate) struct WorkerSlots {
    running: Mutex<usize>,
    finished: Condvar,
    max: usize,
}

/// A claim on one slot, released on drop.
pub(crate) struct WorkerPermit<'a> {
    slots: &'a WorkerSlots,
}

impl WorkerSlots {
    pub(crate) const fn new(max: usize) -> Self {
        Self { running: Mutex::new(0), finished: Condvar::new(), max }
    }

    /// Waits up to `max_wait` for a free slot; `None` when none came free.
    pub(crate) fn acquire(&self, max_wait: Duration) -> Option<WorkerPermit<'_>> {
        let deadline = Instant::now() + max_wait;
        // A poisoned lock only means a holder panicked; the count stays right.
        let mut running = self.running.lock().unwrap_or_else(PoisonError::into_inner);
        while *running >= self.max {
            let left = deadline.saturating_duration_since(Instant::now());
            if left.is_zero() {
                return None;
            }
            running = self.finished.wait_timeout(running, left).unwrap_or_else(PoisonError::into_inner).0;
        }
        *running += 1;

        Some(WorkerPermit { slots: self })
    }
}

impl Drop for WorkerPermit<'_> {
    fn drop(&mut self) {
        let mut running = self.slots.running.lock().unwrap_or_else(PoisonError::into_inner);
        *running -= 1;
        self.slots.finished.notify_one();
    }
}

/// Caps the process's address space, so input that allocates without bound
/// fails in the worker instead of exhausting the machine.
#[cfg(target_os = "linux")]
pub(crate) fn limit_memory(bytes: u64) {
    let limit = rustix::process::Rlimit { current: Some(bytes), maximum: Some(bytes) };
    // Without the cap the worker still runs; the parent's deadline still ends it.
    let _ = rustix::process::setrlimit(rustix::process::Resource::As, limit);
}

/// Other systems have no enforced address-space limit; the deadline ends a
/// runaway worker.
#[cfg(not(target_os = "linux"))]
pub(crate) fn limit_memory(_bytes: u64) {}

/// Why a worker could not place a limit on itself.
#[derive(Debug, thiserror::Error)]
pub(crate) enum SandboxError {
    #[cfg(not(target_os = "linux"))]
    #[error("{feature} is not available on this system")]
    Unsupported { feature: &'static str },

    #[cfg(target_os = "linux")]
    #[error("Landlock is {status:?}")]
    LandlockNotEnforced { status: landlock::LandlockStatus },

    #[cfg(target_os = "linux")]
    #[error("Landlock failed")]
    Landlock(#[from] landlock::RulesetError),

    #[cfg(unix)]
    #[error("failed to set the {resource} limit")]
    Rlimit {
        resource: &'static str,
        #[source]
        source: rustix::io::Errno,
    },
}

impl SandboxError {
    /// The error and its sources on one line, for a worker's reply.
    pub(crate) fn reason(&self) -> String {
        let mut reason = self.to_string();
        let mut source = std::error::Error::source(self);
        while let Some(error) = source {
            reason.push_str(": ");
            reason.push_str(&error.to_string());
            source = error.source();
        }

        reason
    }
}

/// Denies the process every filesystem access right, along with TCP and
/// signals or abstract sockets outside it. Files already open, such as the
/// pipes to the parent, stay usable.
#[cfg(target_os = "linux")]
pub(crate) fn deny_filesystem() -> Result<(), SandboxError> {
    use landlock::ABI;
    use landlock::Access;
    use landlock::AccessFs;
    use landlock::AccessNet;
    use landlock::Ruleset;
    use landlock::RulesetAttr;
    use landlock::RulesetStatus;
    use landlock::Scope;

    // Older kernels enforce the subset they know.
    const ABI_VERSION: ABI = ABI::V6;

    let status = Ruleset::default()
        .handle_access(AccessFs::from_all(ABI_VERSION))
        .and_then(|ruleset| ruleset.handle_access(AccessNet::from_all(ABI_VERSION)))
        .and_then(|ruleset| ruleset.scope(Scope::from_all(ABI_VERSION)))
        .and_then(|ruleset| ruleset.create())
        .and_then(|ruleset| ruleset.restrict_self())?;
    // Every ABI denies opening, listing and creating files when no rule
    // allows them; later ABIs add rights over network, signals and files
    // already open.
    if status.ruleset == RulesetStatus::NotEnforced {
        return Err(SandboxError::LandlockNotEnforced { status: status.landlock });
    }

    Ok(())
}

#[cfg(not(target_os = "linux"))]
pub(crate) fn deny_filesystem() -> Result<(), SandboxError> {
    Err(SandboxError::Unsupported { feature: "Landlock" })
}

/// Lowers the descriptor limit to the three standard streams, so the
/// process can open no file and create no socket of any kind. Descriptors
/// already open stay usable.
#[cfg(unix)]
pub(crate) fn deny_new_descriptors() -> Result<(), SandboxError> {
    const STANDARD_STREAMS: u64 = 3;

    set_limit(rustix::process::Resource::Nofile, "descriptor", STANDARD_STREAMS)
}

#[cfg(not(unix))]
pub(crate) fn deny_new_descriptors() -> Result<(), SandboxError> {
    Err(SandboxError::Unsupported { feature: "a descriptor limit" })
}

/// Lowers the process limit to zero, so the process can start no process
/// that would outlive a kill of it. On Linux this also forbids new threads.
#[cfg(unix)]
pub(crate) fn deny_new_processes() -> Result<(), SandboxError> {
    set_limit(rustix::process::Resource::Nproc, "process", 0)
}

#[cfg(not(unix))]
pub(crate) fn deny_new_processes() -> Result<(), SandboxError> {
    Err(SandboxError::Unsupported { feature: "a process limit" })
}

#[cfg(unix)]
fn set_limit(resource: rustix::process::Resource, name: &'static str, value: u64) -> Result<(), SandboxError> {
    let limit = rustix::process::Rlimit { current: Some(value), maximum: Some(value) };

    rustix::process::setrlimit(resource, limit).map_err(|source| SandboxError::Rlimit { resource: name, source })
}

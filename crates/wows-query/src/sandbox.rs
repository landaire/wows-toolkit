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

/// Other systems have no enforced address-space limit; the parent's memory
/// watch and deadline end a runaway worker.
#[cfg(not(target_os = "linux"))]
pub(crate) fn limit_memory(_bytes: u64) {}

/// Memory process `pid` holds that the system must back: its resident set.
/// `None` once it has exited.
#[cfg(target_os = "linux")]
pub(crate) fn process_memory(pid: u32) -> Option<u64> {
    let status = std::fs::read_to_string(format!("/proc/{pid}/status")).ok()?;
    let kib = status.lines().find_map(|line| line.strip_prefix("VmRSS:"))?.trim().strip_suffix("kB")?;

    kib.trim().parse::<u64>().ok()?.checked_mul(1024)
}

/// Memory process `pid` holds that the system must back: its physical
/// footprint, compressed pages included. `None` once it has exited.
#[cfg(target_os = "macos")]
pub(crate) fn process_memory(pid: u32) -> Option<u64> {
    /// `struct rusage_info_v2` of `<sys/resource.h>`.
    #[repr(C)]
    #[derive(Default)]
    struct RusageInfoV2 {
        uuid: [u8; 16],
        user_time: u64,
        system_time: u64,
        pkg_idle_wkups: u64,
        interrupt_wkups: u64,
        pageins: u64,
        wired_size: u64,
        resident_size: u64,
        phys_footprint: u64,
        proc_start_abstime: u64,
        proc_exit_abstime: u64,
        child_user_time: u64,
        child_system_time: u64,
        child_pkg_idle_wkups: u64,
        child_interrupt_wkups: u64,
        child_pageins: u64,
        child_elapsed_abstime: u64,
        diskio_bytesread: u64,
        diskio_byteswritten: u64,
    }
    const RUSAGE_INFO_V2: i32 = 2;
    // libproc, part of libSystem.
    unsafe extern "C" {
        fn proc_pid_rusage(pid: i32, flavor: i32, buffer: *mut *mut std::ffi::c_void) -> i32;
    }

    let pid = i32::try_from(pid).ok()?;
    let mut info = RusageInfoV2::default();
    // SAFETY: `info` is a `rusage_info_v2`, which the V2 flavor fills, and lives across the call.
    let result = unsafe { proc_pid_rusage(pid, RUSAGE_INFO_V2, (&raw mut info).cast()) };

    (result == 0).then_some(info.phys_footprint)
}

/// Memory process `pid` holds that the system must back: its committed
/// private memory, which counts against the commit limit whether or not it
/// is resident. `None` once it cannot be opened.
#[cfg(windows)]
pub(crate) fn process_memory(pid: u32) -> Option<u64> {
    use windows_sys::Win32::Foundation::CloseHandle;
    use windows_sys::Win32::System::ProcessStatus::GetProcessMemoryInfo;
    use windows_sys::Win32::System::ProcessStatus::PROCESS_MEMORY_COUNTERS_EX;
    use windows_sys::Win32::System::Threading::OpenProcess;
    use windows_sys::Win32::System::Threading::PROCESS_QUERY_LIMITED_INFORMATION;

    // SAFETY: OpenProcess takes no pointers; a null handle is checked below.
    let process = unsafe { OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, pid) };
    if process.is_null() {
        return None;
    }
    // SAFETY: an all-zero PROCESS_MEMORY_COUNTERS_EX is a valid value of the plain-data struct.
    let mut counters: PROCESS_MEMORY_COUNTERS_EX = unsafe { std::mem::zeroed() };
    let size = u32::try_from(std::mem::size_of::<PROCESS_MEMORY_COUNTERS_EX>()).expect("the counters are small");
    counters.cb = size;
    // SAFETY: `process` is an open handle and `counters` is as large as `size` says.
    let read = unsafe { GetProcessMemoryInfo(process, (&raw mut counters).cast(), size) };
    // SAFETY: `process` is open and closed once.
    unsafe { CloseHandle(process) };

    if read == 0 {
        return None;
    }

    u64::try_from(counters.PrivateUsage).ok()
}

#[cfg(not(any(target_os = "linux", target_os = "macos", windows)))]
pub(crate) fn process_memory(_pid: u32) -> Option<u64> {
    None
}

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

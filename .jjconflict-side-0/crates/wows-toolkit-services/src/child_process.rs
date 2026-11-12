//! Starting another program without handing it this process's hardening.

/// Prepares `command` so nothing this process did to itself reaches the child.
///
/// The suppressed error mode is inherited outright, so a genuine load failure
/// in the game or in a relaunched updater would fail silently. That is what the
/// shared helper undoes. No renderer environment is passed on: the desktop
/// front ends choose their adapter in process.
pub fn prepare(command: &mut std::process::Command) -> &mut std::process::Command {
    wows_toolkit_hardening::prepare_child(command, &[])
}

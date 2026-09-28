//! Starting another program without handing it this process's hardening.

/// Prepares `command` so nothing this process did to itself reaches the child.
///
/// The port sets no environment variable for its own renderer: gpui walks the
/// Direct3D 11 adapters itself, with no ladder and no driver pin to leak (see
/// `docs/gpui-port-gaps.md`, item 13). What does leak is the suppressed error
/// mode, which is inherited outright, so a genuine load failure in the game or
/// in the relaunched updater would fail silently. That is what the shared
/// helper undoes.
pub fn prepare(command: &mut std::process::Command) -> &mut std::process::Command {
    wows_toolkit_hardening::prepare_child(command, &[])
}

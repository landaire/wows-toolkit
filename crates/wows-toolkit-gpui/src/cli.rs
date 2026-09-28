//! What this process was asked to do on the command line.
//!
//! The egui app takes flags for the renderer ladder (`--cpu-renderer`,
//! `--gpu-adapter`, `--gpu-safe-mode`, `--list-gpus`), for the process
//! mitigations (`--no-hardening`) and for the updater's cleanup step
//! (`finalize-update --replaced`, plus the bare-path form every released version
//! emits). Each names something this port does not have: it renders through
//! gpui's own backend with no adapter selection, applies no mitigations, and has
//! no updater to finalize. A flag that parsed and then did nothing would be
//! worse than one that is refused by name, so this takes none of them yet.
//!
//! What it does take is `--help` and `--version`, and it refuses anything else
//! rather than starting the app as though the argument had been honoured.

use clap::Parser;

#[derive(Debug, Default, Parser)]
#[command(name = "wows-toolkit-gpui", version, about)]
pub struct Cli {}

/// Reads the process arguments.
///
/// Prints and exits on `--help`, `--version`, and on an argument this build does
/// not take.
pub fn parse() -> Cli {
    Cli::parse()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A flag the egui app takes and this build does not is refused, not
    /// swallowed: the user is told the renderer or the updater is not here yet
    /// rather than left believing the launch honoured it.
    #[test]
    fn a_flag_this_build_does_not_have_is_refused() {
        for flag in ["--cpu-renderer", "--gpu-safe-mode", "--list-gpus", "--no-hardening"] {
            let refused = Cli::try_parse_from(["wows-toolkit-gpui", flag]);
            assert!(refused.is_err(), "{flag} is not taken yet, so it has to be refused");
        }
    }

    /// And a bare launch takes no arguments at all.
    #[test]
    fn a_bare_launch_parses() {
        assert!(Cli::try_parse_from(["wows-toolkit-gpui"]).is_ok());
    }
}

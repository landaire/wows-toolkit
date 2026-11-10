//! What this process was asked to do on the command line.
//!
//! The other front ends' renderer flags are taken as far as they mean anything
//! here. GTK picks the renderer the window draws through, but the armor
//! viewport stands up a `wgpu` device of its own, so `--gpu-adapter`,
//! `--cpu-renderer` and `--list-gpus` steer and report that one.

use std::ffi::OsString;

use clap::Parser;
use wows_toolkit_armor::viewport::device::AdapterChoice;
use wows_toolkit_hardening::Hardening;
use wows_toolkit_hardening::SkipReason;

#[derive(Clone, Debug, Parser)]
#[command(name = "wows-toolkit-nsh", version, about)]
pub struct Cli {
    /// Draw the armor viewport on the CPU rather than a graphics card.
    #[arg(long, conflicts_with = "gpu_adapter")]
    pub cpu_renderer: bool,

    /// Draw the armor viewport on the adapter whose name contains NAME.
    #[arg(long, value_name = "NAME")]
    pub gpu_adapter: Option<String>,

    /// List the adapters the armor viewport can draw on, and exit.
    #[arg(long)]
    pub list_gpus: bool,

    /// Start without the process mitigations.
    #[arg(long)]
    pub no_hardening: bool,
}

impl Cli {
    /// Which adapter the armor viewport's own device should take.
    pub fn adapter_choice(&self) -> AdapterChoice {
        if self.cpu_renderer {
            return AdapterChoice::Cpu;
        }
        match &self.gpu_adapter {
            Some(named) => AdapterChoice::Named(named.clone()),
            None => AdapterChoice::Fastest,
        }
    }

    /// Whether the process mitigations are applied.
    pub fn hardening(&self) -> Hardening {
        if self.no_hardening { Hardening::Skip(SkipReason::CommandLine) } else { Hardening::Apply }
    }
}

/// What this process was asked to do.
#[derive(Debug)]
pub enum Invocation {
    /// Say which adapters the armor viewport can draw on, and exit.
    ListGpus,
    /// Open the window.
    Run(Cli),
}

impl Invocation {
    /// Reads `args`, the program name first.
    pub fn parse_from<I, T>(args: I) -> Result<Self, clap::Error>
    where
        I: IntoIterator<Item = T>,
        T: Into<OsString> + Clone,
    {
        let cli = Cli::try_parse_from(args)?;

        Ok(if cli.list_gpus { Self::ListGpus } else { Self::Run(cli) })
    }
}

/// Reads the process arguments. Prints and exits on `--help`, `--version`,
/// and on an argument this build does not take.
pub fn parse() -> Invocation {
    Invocation::parse_from(std::env::args_os()).unwrap_or_else(|err| err.exit())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn run(args: &[&str]) -> Cli {
        match Invocation::parse_from(std::iter::once("wows-toolkit-nsh").chain(args.iter().copied())) {
            Ok(Invocation::Run(cli)) => cli,
            other => panic!("{args:?} should open the window, got {other:?}"),
        }
    }

    #[test]
    fn a_flag_this_build_does_not_have_is_refused() {
        let refused = Invocation::parse_from(["wows-toolkit-nsh", "--gpu-safe-mode"]).expect_err("an unknown flag");
        assert_eq!(refused.kind(), clap::error::ErrorKind::UnknownArgument);

        let stray = Invocation::parse_from(["wows-toolkit-nsh", "replay.wowsreplay"]).expect_err("a positional");
        assert_eq!(stray.kind(), clap::error::ErrorKind::UnknownArgument);
    }

    #[test]
    fn the_renderer_flags_steer_the_armor_device() {
        let bare = run(&[]);
        assert_eq!(bare.adapter_choice(), AdapterChoice::Fastest);
        assert_eq!(bare.hardening(), Hardening::Apply);

        assert_eq!(run(&["--cpu-renderer"]).adapter_choice(), AdapterChoice::Cpu);
        assert_eq!(run(&["--gpu-adapter", "nvidia"]).adapter_choice(), AdapterChoice::Named("nvidia".into()));
        assert_eq!(run(&["--no-hardening"]).hardening(), Hardening::Skip(SkipReason::CommandLine));

        // Naming an adapter and asking for the CPU answer the same question.
        let both = Invocation::parse_from(["wows-toolkit-nsh", "--cpu-renderer", "--gpu-adapter", "nvidia"])
            .expect_err("one device, one choice");
        assert_eq!(both.kind(), clap::error::ErrorKind::ArgumentConflict);
    }

    #[test]
    fn asking_for_the_adapters_is_not_a_launch() {
        let listed = Invocation::parse_from(["wows-toolkit-nsh", "--list-gpus"]).expect("--list-gpus");
        assert!(matches!(listed, Invocation::ListGpus));
    }

    #[test]
    fn help_and_version_print_rather_than_launch() {
        let help = Invocation::parse_from(["wows-toolkit-nsh", "--help"]).expect_err("--help");
        assert_eq!(help.kind(), clap::error::ErrorKind::DisplayHelp);
        assert!(!help.use_stderr());

        let version = Invocation::parse_from(["wows-toolkit-nsh", "--version"]).expect_err("--version");
        assert_eq!(version.kind(), clap::error::ErrorKind::DisplayVersion);
        assert!(version.render().to_string().contains(env!("CARGO_PKG_VERSION")));
    }
}

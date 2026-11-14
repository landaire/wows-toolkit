use std::process::ExitCode;

use clap::Parser;

/// Parses & processes World of Warships replay files
#[derive(Parser)]
#[command(author = "Lane Kolbly <lane@rscheme.org>, Lander Brandt <landaire>")]
struct Cli {
    #[command(flatten)]
    args: replayshark::cli::Args,
}

fn main() -> ExitCode {
    let cli = Cli::parse();

    match replayshark::cli::run(cli.args) {
        Ok(()) => ExitCode::SUCCESS,
        Err(report) => {
            eprintln!("{report}");
            ExitCode::FAILURE
        }
    }
}

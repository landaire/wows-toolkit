//! `wgc-relay.exe <game exe> [args]`: runs inside the Wine prefix in place of
//! Wargaming.net Game Center. It serves `\\.\pipe\WGC_IPC`, forwards every
//! message to the launcher's loopback listener, starts the game with the WGC
//! launch marker, and exits once the game and everything it started have
//! exited.

#![cfg_attr(windows, windows_subsystem = "windows")]

#[cfg(windows)]
mod error;
#[cfg(windows)]
mod job;
#[cfg(windows)]
mod link;
#[cfg(windows)]
mod pipe;
#[cfg(windows)]
mod registry;

#[cfg(windows)]
fn main() {
    let code = match run() {
        Ok(code) => code,
        Err(err) => {
            eprintln!("wgc-relay: {err}");
            1
        }
    };

    std::process::exit(code as i32);
}

#[cfg(windows)]
fn run() -> Result<u32, error::RelayError> {
    let link = link::LinkConfig::from_env()?;

    let mut args = std::env::args_os().skip(1);
    let game = std::path::PathBuf::from(args.next().ok_or(error::RelayError::MissingGameArgument)?);
    let game_args: Vec<_> = args.collect();

    registry::register_fake_install()?;

    // The first instance exists before the game starts, so its first probe
    // finds a server.
    let first_instance = pipe::PipeInstance::create_first()?;
    std::thread::spawn(move || pipe::serve(first_instance, link));

    job::run_game(&game, &game_args)
}

#[cfg(not(windows))]
fn main() {
    eprintln!("wgc-relay runs inside a Wine prefix; build it for x86_64-pc-windows-gnu");
    std::process::exit(2);
}

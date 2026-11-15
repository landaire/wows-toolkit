//! Resolver latency for the hosts the client names in python.log, timed through getaddrinfo, the
//! resolver Wine's ws2_32 calls.

use std::fmt::Write as _;
use std::io;
use std::net::SocketAddr;
use std::net::ToSocketAddrs;
use std::path::PathBuf;
use std::sync::mpsc;
use std::time::Duration;
use std::time::Instant;

use crate::diagnostics::Clock;
use crate::diagnostics::SinceSignIn;
use crate::diagnostics::StopSignal;
use crate::diagnostics::logs::LogFollower;
use crate::diagnostics::output::CaptureWriter;
use crate::error::CaptureError;

/// Looks up the hosts in turn, one every `INTERVAL`, until stopped.
pub(super) fn probe(
    python_log: PathBuf,
    mut out: CaptureWriter,
    clock: &Clock,
    stop: &StopSignal,
) -> Result<(), CaptureError> {
    const INTERVAL: Duration = Duration::from_millis(500);
    const BACKLOG: u64 = 4 * 1024 * 1024;
    const READ_LIMIT: u64 = 1024 * 1024;
    const MAX_HOSTS: usize = 64;
    const DEADLINE: Duration = Duration::from_secs(5);

    let mut follower = LogFollower::new(python_log.clone(), BACKLOG)
        .map_err(|source| CaptureError::Read { path: python_log.clone(), source })?;
    let mut hosts: Vec<String> = Vec::new();
    let mut next = 0;
    let mut stalled: Option<Lookup> = None;

    out.record(b"t\thost\tmillis\tresult\n")?;
    loop {
        let text =
            follower.read_new(READ_LIMIT).map_err(|source| CaptureError::Read { path: python_log.clone(), source })?;
        for host in hosts_in(&text) {
            if hosts.len() < MAX_HOSTS && !hosts.iter().any(|known| known == host) {
                hosts.push(host.to_owned());
            }
        }

        // getaddrinfo cannot be cancelled, so a lookup past its deadline is left to finish on its
        // own thread and no new lookup starts until it does.
        if let Some(lookup) = &stalled {
            match lookup.result.try_recv() {
                Ok(resolved) => {
                    out.record(lookup.row(resolved).as_bytes())?;
                    stalled = None;
                }
                Err(mpsc::TryRecvError::Empty) => {}
                Err(mpsc::TryRecvError::Disconnected) => stalled = None,
            }
        }

        if stalled.is_none()
            && let Some(host) = hosts.get(next % hosts.len().max(1))
        {
            next += 1;

            let lookup = Lookup::start(host.clone(), clock.now())?;
            match lookup.result.recv_timeout(DEADLINE) {
                Ok(resolved) => out.record(lookup.row(resolved).as_bytes())?,
                Err(mpsc::RecvTimeoutError::Timeout) => {
                    out.record(format!("{}\t{}\t\ttimed out; still waiting\n", lookup.at, lookup.host).as_bytes())?;
                    stalled = Some(lookup);
                }
                Err(mpsc::RecvTimeoutError::Disconnected) => {}
            }
        }

        if stop.wait(INTERVAL) {
            return Ok(());
        }
    }
}

/// One getaddrinfo call on its own thread.
struct Lookup {
    host: String,
    at: SinceSignIn,
    started: Instant,
    result: mpsc::Receiver<io::Result<Vec<SocketAddr>>>,
}

impl Lookup {
    fn start(host: String, at: SinceSignIn) -> Result<Self, CaptureError> {
        const MAX_ADDRESSES: usize = 4;

        let (send, result) = mpsc::channel();
        let query = host.clone();
        std::thread::Builder::new()
            .name("launch-diag-lookup".to_owned())
            .spawn(move || {
                let resolved =
                    (query.as_str(), 443).to_socket_addrs().map(|addresses| addresses.take(MAX_ADDRESSES).collect());
                let _ = send.send(resolved);
            })
            .map_err(|source| CaptureError::Thread { name: "launch-diag-lookup", source })?;

        Ok(Self { host, at, started: Instant::now(), result })
    }

    /// A stalled lookup is polled once per interval, so its time is that coarse.
    fn row(&self, resolved: io::Result<Vec<SocketAddr>>) -> String {
        let took = self.started.elapsed();
        let mut row = format!("{}\t{}\t{:.1}\t", self.at, self.host, took.as_secs_f64() * 1000.0);
        match resolved {
            Ok(addresses) => {
                let addresses: Vec<String> = addresses.iter().map(|address| address.ip().to_string()).collect();
                row.push_str(&addresses.join(","));
            }
            Err(err) => {
                let _ = write!(row, "error: {err}");
            }
        }
        row.push('\n');
        row
    }
}

/// Host names in the URLs and `periphery=` parameters of `text`, IP literals left out.
fn hosts_in(text: &str) -> Vec<&str> {
    const MARKERS: [&str; 3] = ["https://", "http://", "periphery="];

    let mut hosts = Vec::new();
    for marker in MARKERS {
        for (start, _) in text.match_indices(marker) {
            let rest = &text[start + marker.len()..];
            let end =
                rest.find(|ch: char| !(ch.is_ascii_alphanumeric() || ch == '.' || ch == '-')).unwrap_or(rest.len());
            let host = rest[..end].trim_end_matches('.');

            if host.contains('.') && host.chars().any(|ch| ch.is_ascii_alphabetic()) {
                hosts.push(host);
            }
        }
    }

    hosts
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hosts_come_from_urls_and_periphery_parameters() {
        let log = "\
2026-11-15 15:23:07.354: INFO: [Script] [ClientWebSettings] shipyard, ('got service url, service enabled', 'https://dockyard.worldofwarships.eu/'); (, 95950170422700, 62)
2026-11-15 15:23:07.355: INFO: [Script] [ClientWebSettings] metashop, ('got service url, service enabled', 'https://armory.worldofwarships.eu/?utm_source=ingame'); (, 95950170422700, 62)
2026-11-15 15:23:08.592: INFO: [Script] [WebSocketSettings] got service url, service enabled, ('https://ws.worldofwarships.eu/ws',); (, 95950124259486, 49)
2026-11-15 15:23:15.629: INFO: [Script] [Login] Setting wgnr url to \"https://eu.wargaming.net\"; (, 60555212755317, 13)
2026-11-15 15:23:19.473: INFO: [Script] [Login] H:; A:; C:http://csis.worldoftanks.eu/csis/wowseu/?periphery=login1.worldofwarships.eu:20020&periphery=login2.worldofwarships.eu:20020; M:[]
2026-11-15 15:23:20.000: INFO: [Script] [Probe] http://127.0.0.1:37941/ https://localhost/
";

        insta::assert_debug_snapshot!(hosts_in(log), @r#"
        [
            "dockyard.worldofwarships.eu",
            "armory.worldofwarships.eu",
            "ws.worldofwarships.eu",
            "eu.wargaming.net",
            "csis.worldoftanks.eu",
            "login1.worldofwarships.eu",
            "login2.worldofwarships.eu",
        ]
        "#);
    }
}

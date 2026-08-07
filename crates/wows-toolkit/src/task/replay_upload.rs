use std::path::Path;

use tracing::debug;
use tracing::error;
use wows_battle_world::report::BattleReport;
use wowsunpack::game_params::provider::GameMetadataProvider;

use crate::data::settings::DataSharingMode;
use crate::data::shipbuilds::ShipBuildsClient;
use crate::ui::replay_parser::Replay;
use crate::util::build_tracker;

/// Whether a ShipBuilds batch upload consults the sent-replay ledger.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SendReplayCachePolicy {
    UseLedger,
    IgnoreLedger,
}

impl SendReplayCachePolicy {
    pub fn should_attempt(&self, ledger_contains: bool) -> bool {
        matches!(self, Self::IgnoreLedger) || !ledger_contains
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct ReplayCount(pub usize);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SendAllReplaysProgress {
    pub completed: ReplayCount,
    pub total: ReplayCount,
}

impl SendAllReplaysProgress {
    pub fn new(completed: ReplayCount, total: ReplayCount) -> Self {
        Self { completed, total }
    }

    pub fn fraction(&self) -> f32 {
        if self.total.0 == 0 { 0.0 } else { (self.completed.0 as f32 / self.total.0 as f32).clamp(0.0, 1.0) }
    }
}

/// What the background parser should upload for a freshly parsed replay.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReplayUploadAction {
    /// Upload nothing.
    Skip,
    /// Send the per-player build payloads to `/api/ship_builds`.
    BuildData,
    /// Send the raw replay file to `/api/replays`.
    RawReplay,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ShipBuildsUploadOutcome {
    Skipped,
    Sent,
    TransientFailure,
}

/// Decide what to upload. `self_confirmed_non_test` must be `true` only when the
/// self player's ship is positively known not to be a test ship; any
/// uncertainty is `false`, which keeps a possible test-ship replay off
/// `/api/replays` (hard rule) by falling back to build data.
pub fn decide_upload_action(
    mode: DataSharingMode,
    is_valid_game_type: bool,
    self_confirmed_non_test: bool,
) -> ReplayUploadAction {
    match mode {
        DataSharingMode::Off => ReplayUploadAction::Skip,
        DataSharingMode::BuildData if is_valid_game_type => ReplayUploadAction::BuildData,
        DataSharingMode::Replays if is_valid_game_type => {
            if self_confirmed_non_test {
                ReplayUploadAction::RawReplay
            } else {
                ReplayUploadAction::BuildData
            }
        }
        _ => ReplayUploadAction::Skip,
    }
}

pub(crate) fn upload_parsed_replay(
    path: &Path,
    replay: &Replay,
    report: &BattleReport,
    metadata: &GameMetadataProvider,
    mode: DataSharingMode,
    client: &ShipBuildsClient,
) -> ShipBuildsUploadOutcome {
    let Some(game_type) = replay.replay_file.meta.gameType.as_ref() else {
        debug!("replay {:?} has no game type and is not eligible for upload", path);
        return ShipBuildsUploadOutcome::Skipped;
    };
    let replay_version = wowsunpack::data::Version::from_client_exe(&replay.replay_file.meta.clientVersionFromExe);
    let battle_type = wowsunpack::game_types::BattleType::from_value(&game_type, replay_version);
    let is_valid_game_type = matches!(
        battle_type.known(),
        Some(wowsunpack::game_types::BattleType::Random | wowsunpack::game_types::BattleType::Ranked)
    );
    if !is_valid_game_type {
        debug!("game type is: {}", &game_type);
    }

    let self_confirmed_non_test = report
        .players()
        .iter()
        .find(|player| player.relation().is_self())
        .and_then(|player| player.vehicle().vehicle())
        .map(|vehicle| !vehicle.is_test_ship())
        .unwrap_or(false);

    match decide_upload_action(mode, is_valid_game_type, self_confirmed_non_test) {
        ReplayUploadAction::Skip => ShipBuildsUploadOutcome::Skipped,
        ReplayUploadAction::BuildData => {
            let mut requests = Vec::new();
            for player in report.players().iter().filter(|player| !player.is_bot()) {
                let Some(realm) = player.initial_state().realm() else {
                    error!("failed to build ShipBuilds payload for replay {:?}: player realm is missing", path);
                    return ShipBuildsUploadOutcome::TransientFailure;
                };
                #[cfg(not(feature = "shipbuilds_debugging"))]
                let url = "https://shipbuilds.com/api/ship_builds";
                #[cfg(feature = "shipbuilds_debugging")]
                let url = "http://192.168.1.215:3000/api/ship_builds";

                if let Some(payload) = build_tracker::BuildTrackerPayload::build_from(
                    player,
                    realm.to_string(),
                    report.version(),
                    game_type.clone(),
                    metadata,
                ) {
                    requests.push(client.http().post(url).json(&payload));
                } else {
                    error!("failed to build ShipBuilds payload for replay {:?}: player vehicle is missing", path);
                    return ShipBuildsUploadOutcome::TransientFailure;
                }
            }
            let outcome = send_shipbuilds_requests(path, requests);
            if outcome == ShipBuildsUploadOutcome::Sent {
                debug!("Successfully sent all builds");
            }
            outcome
        }
        ReplayUploadAction::RawReplay => {
            #[cfg(not(feature = "shipbuilds_debugging"))]
            let url = "https://shipbuilds.com/api/replays";
            #[cfg(feature = "shipbuilds_debugging")]
            let url = "http://192.168.1.215:3000/api/replays";

            match std::fs::read(path) {
                Ok(bytes) => send_shipbuilds_requests(
                    path,
                    vec![
                        client
                            .http()
                            .post(url)
                            .header(reqwest::header::CONTENT_TYPE, "application/octet-stream")
                            .body(bytes),
                    ],
                ),
                Err(error) => {
                    error!("failed to read replay file for upload {:?}: {:?}", path, error);
                    ShipBuildsUploadOutcome::TransientFailure
                }
            }
        }
    }
}

fn send_shipbuilds_requests(path: &Path, requests: Vec<reqwest::blocking::RequestBuilder>) -> ShipBuildsUploadOutcome {
    if requests.is_empty() {
        error!("no valid ShipBuilds payloads for replay {:?}", path);
        return ShipBuildsUploadOutcome::TransientFailure;
    }

    for request in requests {
        match request.send() {
            Ok(response) if response.status().is_success() => {}
            Ok(response) => {
                error!("ShipBuilds rejected replay {:?} with HTTP {}", path, response.status());
                return ShipBuildsUploadOutcome::TransientFailure;
            }
            Err(error) => {
                error!("error sending replay {:?}: {:?}", path, error);
                return ShipBuildsUploadOutcome::TransientFailure;
            }
        }
    }

    ShipBuildsUploadOutcome::Sent
}

#[cfg(test)]
mod tests {
    use std::io::Read;
    use std::io::Write;
    use std::net::TcpListener;
    use std::sync::mpsc;
    use std::time::Duration;

    use super::*;

    fn status_server(statuses: &[u16]) -> (String, std::thread::JoinHandle<()>) {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let address = listener.local_addr().unwrap();
        let statuses = statuses.to_vec();
        let handle = std::thread::spawn(move || {
            for status in statuses {
                let (mut stream, _) = listener.accept().unwrap();
                let mut request = [0; 4096];
                let _ = stream.read(&mut request).unwrap();
                let reason = if (200..300).contains(&status) { "OK" } else { "Error" };
                write!(stream, "HTTP/1.1 {status} {reason}\r\nContent-Length: 0\r\nConnection: close\r\n\r\n").unwrap();
            }
        });
        (format!("http://{address}"), handle)
    }

    #[test]
    fn every_2xx_request_is_a_completed_send() {
        let (url, server) = status_server(&[200, 204]);
        let client = reqwest::blocking::Client::new();

        let outcome = send_shipbuilds_requests(
            Path::new("success.wowsreplay"),
            vec![client.post(&url).body("first"), client.post(&url).body("second")],
        );

        server.join().unwrap();
        assert_eq!(outcome, ShipBuildsUploadOutcome::Sent);
    }

    #[test]
    fn an_http_error_response_is_retryable() {
        for status in [302, 400, 500] {
            let (url, server) = status_server(&[status]);
            let client = reqwest::blocking::Client::new();

            let outcome =
                send_shipbuilds_requests(Path::new("http-error.wowsreplay"), vec![client.post(&url).body("request")]);

            server.join().unwrap();
            assert_eq!(outcome, ShipBuildsUploadOutcome::TransientFailure, "HTTP {status}");
        }
    }

    #[test]
    fn a_location_redirect_is_not_followed_or_sent() {
        let target_listener = TcpListener::bind("127.0.0.1:0").unwrap();
        target_listener.set_nonblocking(true).unwrap();
        let target_url = format!("http://{}", target_listener.local_addr().unwrap());
        let (stop_target_tx, stop_target_rx) = mpsc::channel();
        let (target_hit_tx, target_hit_rx) = mpsc::channel();
        let target_server = std::thread::spawn(move || {
            loop {
                match target_listener.accept() {
                    Ok((mut stream, _)) => {
                        stream.set_nonblocking(false).unwrap();
                        let mut request = [0; 4096];
                        let _ = stream.read(&mut request).unwrap();
                        write!(stream, "HTTP/1.1 200 OK\r\nContent-Length: 0\r\nConnection: close\r\n\r\n").unwrap();
                        target_hit_tx.send(true).unwrap();
                        return;
                    }
                    Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                        if stop_target_rx.try_recv().is_ok() {
                            target_hit_tx.send(false).unwrap();
                            return;
                        }
                        std::thread::yield_now();
                    }
                    Err(error) => panic!("target accept failed: {error}"),
                }
            }
        });

        let redirect_listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let redirect_url = format!("http://{}", redirect_listener.local_addr().unwrap());
        let redirect_server = std::thread::spawn(move || {
            let (mut stream, _) = redirect_listener.accept().unwrap();
            let mut request = [0; 4096];
            let _ = stream.read(&mut request).unwrap();
            write!(
                stream,
                "HTTP/1.1 302 Found\r\nLocation: {target_url}\r\nContent-Length: 0\r\nConnection: close\r\n\r\n"
            )
            .unwrap();
        });
        let client = ShipBuildsClient::new().unwrap();

        let outcome = send_shipbuilds_requests(
            Path::new("redirect.wowsreplay"),
            vec![client.http().post(redirect_url).body("request")],
        );

        redirect_server.join().unwrap();
        let _ = stop_target_tx.send(());
        target_server.join().unwrap();
        assert_eq!(outcome, ShipBuildsUploadOutcome::TransientFailure);
        assert!(!target_hit_rx.recv_timeout(Duration::from_secs(5)).unwrap());
    }

    #[test]
    fn a_partial_build_upload_is_retryable() {
        let (url, server) = status_server(&[200, 500]);
        let client = reqwest::blocking::Client::new();

        let outcome = send_shipbuilds_requests(
            Path::new("partial.wowsreplay"),
            vec![client.post(&url).body("first"), client.post(&url).body("second")],
        );

        server.join().unwrap();
        assert_eq!(outcome, ShipBuildsUploadOutcome::TransientFailure);
    }

    #[test]
    fn an_empty_build_request_set_is_retryable() {
        let outcome = send_shipbuilds_requests(Path::new("empty.wowsreplay"), Vec::new());

        assert_eq!(outcome, ShipBuildsUploadOutcome::TransientFailure);
    }

    #[test]
    fn a_request_error_is_retryable() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let url = format!("http://{}", listener.local_addr().unwrap());
        drop(listener);
        let client = reqwest::blocking::Client::new();

        let outcome =
            send_shipbuilds_requests(Path::new("request-error.wowsreplay"), vec![client.post(url).body("request")]);

        assert_eq!(outcome, ShipBuildsUploadOutcome::TransientFailure);
    }

    #[test]
    fn a_timeout_is_retryable() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let url = format!("http://{}", listener.local_addr().unwrap());
        let (release_tx, release_rx) = mpsc::channel();
        let server = std::thread::spawn(move || {
            let (_stream, _) = listener.accept().unwrap();
            let _ = release_rx.recv();
        });
        let client = reqwest::blocking::Client::builder().timeout(Duration::from_millis(25)).build().unwrap();

        let outcome = send_shipbuilds_requests(Path::new("timeout.wowsreplay"), vec![client.post(url).body("request")]);

        let _ = release_tx.send(());
        server.join().unwrap();
        assert_eq!(outcome, ShipBuildsUploadOutcome::TransientFailure);
    }

    #[test]
    fn normal_policy_skips_ledger_entries_but_debug_policy_does_not() {
        assert!(!SendReplayCachePolicy::UseLedger.should_attempt(true));
        assert!(SendReplayCachePolicy::UseLedger.should_attempt(false));
        assert!(SendReplayCachePolicy::IgnoreLedger.should_attempt(true));
    }

    #[test]
    fn empty_progress_has_a_safe_fraction() {
        let progress = SendAllReplaysProgress::new(ReplayCount(0), ReplayCount(0));
        assert_eq!(progress.fraction(), 0.0);
    }

    #[test]
    fn off_never_uploads() {
        for valid in [true, false] {
            for non_test in [true, false] {
                assert_eq!(decide_upload_action(DataSharingMode::Off, valid, non_test), ReplayUploadAction::Skip);
            }
        }
    }

    #[test]
    fn invalid_game_type_never_uploads() {
        assert_eq!(decide_upload_action(DataSharingMode::BuildData, false, true), ReplayUploadAction::Skip);
        assert_eq!(decide_upload_action(DataSharingMode::Replays, false, true), ReplayUploadAction::Skip);
    }

    #[test]
    fn build_data_mode_sends_build_data() {
        assert_eq!(decide_upload_action(DataSharingMode::BuildData, true, true), ReplayUploadAction::BuildData);
        assert_eq!(decide_upload_action(DataSharingMode::BuildData, true, false), ReplayUploadAction::BuildData);
    }

    #[test]
    fn replays_mode_sends_raw_only_when_confirmed_non_test() {
        assert_eq!(decide_upload_action(DataSharingMode::Replays, true, true), ReplayUploadAction::RawReplay);
    }

    #[test]
    fn replays_mode_falls_back_to_build_data_when_test_or_indeterminate() {
        assert_eq!(decide_upload_action(DataSharingMode::Replays, true, false), ReplayUploadAction::BuildData);
    }
}

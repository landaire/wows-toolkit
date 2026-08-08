use std::path::Path;

use tracing::debug;
use tracing::error;
use wows_battle_world::report::BattleReport;
use wowsunpack::game_params::provider::GameMetadataProvider;

use crate::data::replay_reconcile::RawUploadDeadline;
use crate::data::settings::DataSharingMode;
use crate::data::shipbuilds::ShipBuildsClient;
use crate::data::wows_data::ReplayBytes;
use crate::ui::replay_parser::Replay;
use crate::util::build_tracker;

/// Grace window measured from the replay file's mtime. When it lapses, a
/// results-less replay is uploaded as-is; the file is not going to improve.
pub const RAW_UPLOAD_GRACE: jiff::SignedDuration = jiff::SignedDuration::from_secs(20 * 60);

/// Whether the raw replay file is ready for `/api/replays`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RawReplaySnapshotState {
    /// End-of-battle results are in the file.
    Complete,
    /// No results yet; hold the upload until they arrive or the deadline fires.
    IncompleteWithinGrace { deadline: RawUploadDeadline },
    /// No results and the grace window has lapsed.
    IncompleteGraceLapsed,
}

pub fn raw_replay_snapshot_state(
    results_available: bool,
    mtime: Option<jiff::Timestamp>,
    now: jiff::Timestamp,
) -> RawReplaySnapshotState {
    if results_available {
        return RawReplaySnapshotState::Complete;
    }
    // A missing mtime anchors the window at `now`: the fallback upload is
    // delayed by at most one full window, never issued early.
    let anchor = mtime.unwrap_or(now);
    // Saturate: an anchor close enough to jiff's range edge to overflow is
    // garbage, and holding the upload for results is the conservative reading.
    let deadline = anchor.checked_add(RAW_UPLOAD_GRACE).unwrap_or(jiff::Timestamp::MAX);
    if deadline <= now {
        RawReplaySnapshotState::IncompleteGraceLapsed
    } else {
        RawReplaySnapshotState::IncompleteWithinGrace { deadline: RawUploadDeadline(deadline) }
    }
}

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
    Skip(ReplayUploadSkipReason),
    /// Send the per-player build payloads to `/api/ship_builds`.
    BuildData,
    /// Send the raw replay file to `/api/replays`.
    RawReplay,
    /// Send nothing yet; retry when results arrive or the deadline fires.
    AwaitResults { deadline: RawUploadDeadline },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReplayUploadSkipReason {
    SharingDisabled,
    IneligibleGameType,
    /// Replays mode only shares raw replays, and a possible test-ship battle
    /// must stay off `/api/replays`.
    PossibleTestShip,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ShipBuildsUploadOutcome {
    Skipped(ReplayUploadSkipReason),
    Sent,
    AwaitingResults { deadline: RawUploadDeadline },
    TransientFailure,
}

/// Decide what to upload. `self_confirmed_non_test` must be `true` only when the
/// self player's ship is positively known not to be a test ship; any
/// uncertainty is `false`, which keeps a possible test-ship replay off
/// `/api/replays` (hard rule). Replays mode never falls back to build data;
/// the two payloads are mutually exclusive. A raw upload additionally waits
/// for end-of-battle results until the grace deadline fires.
pub fn decide_upload_action(
    mode: DataSharingMode,
    is_valid_game_type: bool,
    self_confirmed_non_test: bool,
    raw_snapshot: RawReplaySnapshotState,
) -> ReplayUploadAction {
    if !is_valid_game_type {
        return ReplayUploadAction::Skip(ReplayUploadSkipReason::IneligibleGameType);
    }

    match mode {
        DataSharingMode::Off => ReplayUploadAction::Skip(ReplayUploadSkipReason::SharingDisabled),
        DataSharingMode::BuildData => ReplayUploadAction::BuildData,
        DataSharingMode::Replays => {
            if !self_confirmed_non_test {
                ReplayUploadAction::Skip(ReplayUploadSkipReason::PossibleTestShip)
            } else {
                match raw_snapshot {
                    RawReplaySnapshotState::Complete | RawReplaySnapshotState::IncompleteGraceLapsed => {
                        ReplayUploadAction::RawReplay
                    }
                    RawReplaySnapshotState::IncompleteWithinGrace { deadline } => {
                        ReplayUploadAction::AwaitResults { deadline }
                    }
                }
            }
        }
    }
}

pub(crate) fn build_shipbuilds_payload<T>(
    replay_path: Option<&Path>,
    player_id: wows_replays::types::AccountId,
    realm: Option<&str>,
    build: impl FnOnce(String) -> Option<T>,
) -> Option<T> {
    let Some(realm) = realm.filter(|realm| !realm.trim().is_empty()) else {
        error!(?replay_path, %player_id, "skipping ShipBuilds player because realm is missing or empty");
        return None;
    };

    let payload = build(realm.to_owned());
    if payload.is_none() {
        error!(?replay_path, %player_id, "skipping ShipBuilds player because build data is unavailable");
    }
    payload
}

pub(crate) fn send_shipbuilds_payloads<T: serde::Serialize>(
    replay_path: Option<&Path>,
    payloads: impl IntoIterator<Item = Option<T>>,
    client: &ShipBuildsClient,
    url: &str,
) -> ShipBuildsUploadOutcome {
    let requests = payloads.into_iter().flatten().map(|payload| client.http().post(url).json(&payload)).collect();
    send_shipbuilds_requests_for_replay(replay_path, requests)
}

pub(crate) fn upload_parsed_replay(
    path: &Path,
    replay: &Replay,
    report: &BattleReport,
    metadata: &GameMetadataProvider,
    mode: DataSharingMode,
    client: &ShipBuildsClient,
    replay_bytes: ReplayBytes,
) -> ShipBuildsUploadOutcome {
    let Some(game_type) = replay.replay_file.meta.gameType.as_ref() else {
        debug!("replay {:?} has no game type and is not eligible for upload", path);
        return ShipBuildsUploadOutcome::Skipped(ReplayUploadSkipReason::IneligibleGameType);
    };
    let replay_version = wowsunpack::data::Version::from_client_exe(&replay.replay_file.meta.clientVersionFromExe);
    let battle_type = wowsunpack::game_types::BattleType::from_value(game_type, replay_version);
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

    let mtime = std::fs::metadata(path).and_then(|meta| meta.modified()).ok().and_then(|mtime| {
        // A file time outside jiff's range is garbage; treat it as missing so
        // the grace window anchors at now.
        jiff::Timestamp::try_from(mtime).ok()
    });
    let raw_snapshot = raw_replay_snapshot_state(report.battle_results().is_some(), mtime, jiff::Timestamp::now());

    match decide_upload_action(mode, is_valid_game_type, self_confirmed_non_test, raw_snapshot) {
        ReplayUploadAction::Skip(reason) => ShipBuildsUploadOutcome::Skipped(reason),
        ReplayUploadAction::AwaitResults { deadline } => {
            debug!("holding raw upload for {:?} until results or {:?}", path, deadline);
            ShipBuildsUploadOutcome::AwaitingResults { deadline }
        }
        ReplayUploadAction::BuildData => {
            #[cfg(not(feature = "shipbuilds_debugging"))]
            let url = "https://shipbuilds.com/api/ship_builds";
            #[cfg(feature = "shipbuilds_debugging")]
            let url = "http://192.168.1.215:3000/api/ship_builds";

            let payloads = report.players().iter().filter(|player| !player.is_bot()).map(|player| {
                build_shipbuilds_payload(
                    Some(path),
                    player.initial_state().db_id(),
                    player.initial_state().realm(),
                    |realm| {
                        build_tracker::BuildTrackerPayload::build_from(
                            player,
                            realm,
                            report.version(),
                            game_type.clone(),
                            metadata,
                        )
                    },
                )
            });
            let outcome = send_shipbuilds_payloads(Some(path), payloads, client, url);
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

            send_raw_replay_snapshot(path, replay_bytes, client, url)
        }
    }
}

fn send_raw_replay_snapshot(
    path: &Path,
    replay_bytes: ReplayBytes,
    client: &ShipBuildsClient,
    url: &str,
) -> ShipBuildsUploadOutcome {
    send_shipbuilds_requests(
        path,
        vec![
            client
                .http()
                .post(url)
                .header(reqwest::header::CONTENT_TYPE, "application/octet-stream")
                .body(replay_bytes.into_vec()),
        ],
    )
}

fn send_shipbuilds_requests(path: &Path, requests: Vec<reqwest::blocking::RequestBuilder>) -> ShipBuildsUploadOutcome {
    send_shipbuilds_requests_for_replay(Some(path), requests)
}

pub(crate) fn send_shipbuilds_requests_for_replay(
    replay_path: Option<&Path>,
    requests: Vec<reqwest::blocking::RequestBuilder>,
) -> ShipBuildsUploadOutcome {
    if requests.is_empty() {
        error!(?replay_path, "no valid ShipBuilds payloads for replay");
        return ShipBuildsUploadOutcome::TransientFailure;
    }

    for request in requests {
        match request.send() {
            Ok(response) if response.status().is_success() => {}
            Ok(response) => {
                error!(?replay_path, status = %response.status(), "ShipBuilds rejected replay");
                return ShipBuildsUploadOutcome::TransientFailure;
            }
            Err(error) => {
                error!(?replay_path, ?error, "error sending replay");
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
    fn a_missing_realm_skips_payload_construction_for_that_player() {
        let payload = build_shipbuilds_payload(
            Some(Path::new("missing-realm.wowsreplay")),
            wows_replays::types::AccountId(42),
            None,
            |_| -> Option<serde_json::Value> { panic!("payload builder must not receive an invented realm") },
        );

        assert!(payload.is_none());
    }

    #[test]
    fn an_empty_realm_skips_payload_construction_for_that_player() {
        let payload = build_shipbuilds_payload(
            Some(Path::new("empty-realm.wowsreplay")),
            wows_replays::types::AccountId(42),
            Some(""),
            |_| -> Option<serde_json::Value> { panic!("payload builder must not receive an empty realm") },
        );

        assert!(payload.is_none());
    }

    #[test]
    fn a_missing_payload_candidate_is_not_sent_when_a_valid_candidate_remains() {
        let (url, server) = status_server(&[204]);
        let client = ShipBuildsClient::new().unwrap();

        let outcome = send_shipbuilds_payloads(
            Some(Path::new("partial-payloads.wowsreplay")),
            vec![None, Some(serde_json::json!({ "realm": "NA" }))],
            &client,
            &url,
        );

        server.join().unwrap();
        assert_eq!(outcome, ShipBuildsUploadOutcome::Sent);
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
    fn raw_upload_uses_the_same_file_snapshot_that_was_parsed() {
        let fixture = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("..")
            .join("..")
            .join("tests")
            .join("fixtures")
            .join("replays")
            .join("20220124_194638_PISB105-Conte-di-Cavour_22_tierra_del_fuego.wowsreplay");
        let original = std::fs::read(&fixture).unwrap();
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("race.wowsreplay");
        std::fs::write(&path, &original).unwrap();
        let snapshot = crate::data::wows_data::ReplayFileSnapshot::read(&path).unwrap();
        assert!(!snapshot.replay_file.meta.clientVersionFromExe.is_empty());
        std::fs::write(&path, b"replacement bytes").unwrap();

        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let url = format!("http://{}", listener.local_addr().unwrap());
        let (body_tx, body_rx) = mpsc::channel();
        let server = std::thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            let mut request = Vec::new();
            let mut buffer = [0_u8; 16 * 1024];
            let (header_end, content_length) = loop {
                let read = stream.read(&mut buffer).unwrap();
                assert_ne!(read, 0, "request ended before its body arrived");
                request.extend_from_slice(&buffer[..read]);
                let Some(header_end) = request.windows(4).position(|window| window == b"\r\n\r\n") else {
                    continue;
                };
                let header_end = header_end + 4;
                let headers = std::str::from_utf8(&request[..header_end]).unwrap();
                let content_length = headers
                    .lines()
                    .find_map(|line| {
                        let (name, value) = line.split_once(':')?;
                        name.eq_ignore_ascii_case("content-length").then(|| value.trim().parse::<usize>().unwrap())
                    })
                    .expect("raw replay request has a content length");
                break (header_end, content_length);
            };
            while request.len() < header_end + content_length {
                let read = stream.read(&mut buffer).unwrap();
                assert_ne!(read, 0, "request ended before its declared body length");
                request.extend_from_slice(&buffer[..read]);
            }
            body_tx.send(request[header_end..header_end + content_length].to_vec()).unwrap();
            write!(stream, "HTTP/1.1 204 No Content\r\nContent-Length: 0\r\nConnection: close\r\n\r\n").unwrap();
        });
        let client = ShipBuildsClient::new().unwrap();

        let outcome = send_raw_replay_snapshot(&path, snapshot.bytes, &client, &url);

        server.join().unwrap();
        assert_eq!(outcome, ShipBuildsUploadOutcome::Sent);
        assert_eq!(body_rx.recv_timeout(Duration::from_secs(5)).unwrap(), original);
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

    fn timestamp(second: i64) -> jiff::Timestamp {
        jiff::Timestamp::from_second(second).unwrap()
    }

    fn within_grace() -> RawReplaySnapshotState {
        RawReplaySnapshotState::IncompleteWithinGrace { deadline: RawUploadDeadline(timestamp(1_200)) }
    }

    #[test]
    fn off_never_uploads() {
        for valid in [true, false] {
            for non_test in [true, false] {
                let expected = if valid {
                    ReplayUploadAction::Skip(ReplayUploadSkipReason::SharingDisabled)
                } else {
                    ReplayUploadAction::Skip(ReplayUploadSkipReason::IneligibleGameType)
                };
                assert_eq!(
                    decide_upload_action(DataSharingMode::Off, valid, non_test, RawReplaySnapshotState::Complete),
                    expected
                );
            }
        }
    }

    #[test]
    fn invalid_game_type_never_uploads() {
        assert_eq!(
            decide_upload_action(DataSharingMode::BuildData, false, true, RawReplaySnapshotState::Complete),
            ReplayUploadAction::Skip(ReplayUploadSkipReason::IneligibleGameType)
        );
        assert_eq!(
            decide_upload_action(DataSharingMode::Replays, false, true, RawReplaySnapshotState::Complete),
            ReplayUploadAction::Skip(ReplayUploadSkipReason::IneligibleGameType)
        );
    }

    #[test]
    fn build_data_mode_sends_build_data_regardless_of_completeness() {
        for snapshot in
            [RawReplaySnapshotState::Complete, within_grace(), RawReplaySnapshotState::IncompleteGraceLapsed]
        {
            assert_eq!(
                decide_upload_action(DataSharingMode::BuildData, true, true, snapshot),
                ReplayUploadAction::BuildData
            );
            assert_eq!(
                decide_upload_action(DataSharingMode::BuildData, true, false, snapshot),
                ReplayUploadAction::BuildData
            );
        }
    }

    #[test]
    fn replays_mode_sends_raw_when_confirmed_non_test_and_complete() {
        assert_eq!(
            decide_upload_action(DataSharingMode::Replays, true, true, RawReplaySnapshotState::Complete),
            ReplayUploadAction::RawReplay
        );
    }

    #[test]
    fn replays_mode_never_sends_builds_for_test_or_indeterminate_ships() {
        for snapshot in
            [RawReplaySnapshotState::Complete, within_grace(), RawReplaySnapshotState::IncompleteGraceLapsed]
        {
            assert_eq!(
                decide_upload_action(DataSharingMode::Replays, true, false, snapshot),
                ReplayUploadAction::Skip(ReplayUploadSkipReason::PossibleTestShip)
            );
        }
    }

    #[test]
    fn replays_mode_defers_an_incomplete_replay_within_grace() {
        let deadline = RawUploadDeadline(timestamp(1_200));
        assert_eq!(
            decide_upload_action(
                DataSharingMode::Replays,
                true,
                true,
                RawReplaySnapshotState::IncompleteWithinGrace { deadline }
            ),
            ReplayUploadAction::AwaitResults { deadline }
        );
    }

    #[test]
    fn replays_mode_uploads_an_incomplete_replay_after_grace() {
        assert_eq!(
            decide_upload_action(DataSharingMode::Replays, true, true, RawReplaySnapshotState::IncompleteGraceLapsed),
            ReplayUploadAction::RawReplay
        );
    }

    #[test]
    fn results_in_the_file_are_complete_regardless_of_mtime() {
        for mtime in [None, Some(timestamp(0)), Some(timestamp(10_000_000))] {
            assert_eq!(raw_replay_snapshot_state(true, mtime, timestamp(100)), RawReplaySnapshotState::Complete);
        }
    }

    #[test]
    fn a_fresh_results_less_file_waits_until_mtime_plus_grace() {
        let mtime = timestamp(1_000);
        let now = timestamp(1_060);
        assert_eq!(
            raw_replay_snapshot_state(false, Some(mtime), now),
            RawReplaySnapshotState::IncompleteWithinGrace {
                deadline: RawUploadDeadline(timestamp(1_000 + 20 * 60))
            }
        );
    }

    #[test]
    fn a_stale_results_less_file_has_lapsed() {
        let mtime = timestamp(1_000);
        let now = timestamp(1_000 + 20 * 60);
        assert_eq!(raw_replay_snapshot_state(false, Some(mtime), now), RawReplaySnapshotState::IncompleteGraceLapsed);
    }

    #[test]
    fn a_missing_mtime_anchors_the_grace_window_at_now() {
        let now = timestamp(5_000);
        assert_eq!(
            raw_replay_snapshot_state(false, None, now),
            RawReplaySnapshotState::IncompleteWithinGrace {
                deadline: RawUploadDeadline(timestamp(5_000 + 20 * 60))
            }
        );
    }
}

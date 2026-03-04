//! Wire protocol for collaborative replay sessions (mesh topology).
//!
//! Re-exports all types from `wt_collab_protocol::protocol` and adds
//! iroh-specific stream framing helpers.

// Re-export everything from the protocol crate.
pub use wt_collab_protocol::protocol::*;

// ─── CollabRenderOptions desktop-specific conversion functions ──────────────

/// Convert from the persisted settings format.
pub fn collab_render_options_from_saved(s: &crate::settings::SavedRenderOptions) -> CollabRenderOptions {
    CollabRenderOptions {
        show_hp_bars: s.show_hp_bars,
        show_tracers: s.show_tracers,
        show_torpedoes: s.show_torpedoes,
        show_planes: s.show_planes,
        show_smoke: s.show_smoke,
        show_score: s.show_score,
        show_timer: s.show_timer,
        show_kill_feed: s.show_kill_feed,
        show_player_names: s.show_player_names,
        show_ship_names: s.show_ship_names,
        show_capture_points: s.show_capture_points,
        show_buildings: s.show_buildings,
        show_turret_direction: s.show_turret_direction,
        show_consumables: s.show_consumables,
        show_dead_ships: s.show_dead_ships,
        show_dead_ship_names: s.show_dead_ship_names,
        show_armament: s.show_armament,
        show_trails: s.show_trails,
        show_dead_trails: s.show_dead_trails,
        show_speed_trails: s.show_speed_trails,
        show_battle_result: s.show_battle_result,
        show_buffs: s.show_buffs,
        show_ship_config: s.show_ship_config,
        show_chat: s.show_chat,
        show_advantage: s.show_advantage,
        show_score_timer: s.show_score_timer,
        show_self_detection_range: s.show_self_detection_range,
        show_self_main_battery_range: s.show_self_main_battery_range,
        show_self_secondary_range: s.show_self_secondary_range,
        show_self_torpedo_range: s.show_self_torpedo_range,
        show_self_radar_range: s.show_self_radar_range,
        show_self_hydro_range: s.show_self_hydro_range,
    }
}

/// Build from a `RenderOptions` and the separate `show_dead_ships` flag.
pub fn collab_render_options_from_render_options(
    opts: &wows_minimap_renderer::renderer::RenderOptions,
    show_dead_ships: bool,
) -> CollabRenderOptions {
    CollabRenderOptions {
        show_hp_bars: opts.show_hp_bars,
        show_tracers: opts.show_tracers,
        show_torpedoes: opts.show_torpedoes,
        show_planes: opts.show_planes,
        show_smoke: opts.show_smoke,
        show_score: opts.show_score,
        show_timer: opts.show_timer,
        show_kill_feed: opts.show_kill_feed,
        show_player_names: opts.show_player_names,
        show_ship_names: opts.show_ship_names,
        show_capture_points: opts.show_capture_points,
        show_buildings: opts.show_buildings,
        show_turret_direction: opts.show_turret_direction,
        show_consumables: opts.show_consumables,
        show_dead_ships,
        show_dead_ship_names: opts.show_dead_ship_names,
        show_armament: opts.show_armament,
        show_trails: opts.show_trails,
        show_dead_trails: opts.show_dead_trails,
        show_speed_trails: opts.show_speed_trails,
        show_battle_result: opts.show_battle_result,
        show_buffs: opts.show_buffs,
        show_ship_config: opts.show_ship_config,
        show_chat: opts.show_chat,
        show_advantage: opts.show_advantage,
        show_score_timer: opts.show_score_timer,
        show_self_detection_range: false,
        show_self_main_battery_range: false,
        show_self_secondary_range: false,
        show_self_torpedo_range: false,
        show_self_radar_range: false,
        show_self_hydro_range: false,
    }
}

// ─── iroh-specific wire framing helpers ─────────────────────────────────────

/// Write a length-prefixed rkyv-serialized `PeerMessage` to a QUIC send stream.
pub async fn write_peer_message(
    send: &mut iroh::endpoint::SendStream,
    msg: &PeerMessage,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let framed = serialize_message(msg)?;
    send.write_all(&framed).await.map_err(|e| format!("write: {e}"))?;
    Ok(())
}

/// Serialize a `PeerMessage` to length-prefixed rkyv bytes (for broadcast).
///
/// This is an alias for `wt_collab_protocol::protocol::serialize_message` kept
/// for backward compatibility with callers using the old name.
pub fn frame_peer_message(msg: &PeerMessage) -> Result<Vec<u8>, Box<dyn std::error::Error + Send + Sync>> {
    serialize_message(msg)
}

/// Read a length-prefixed rkyv `PeerMessage` from a QUIC receive stream.
///
/// Returns `None` if the stream is cleanly closed.
pub async fn read_peer_message(
    recv: &mut iroh::endpoint::RecvStream,
    max_size: usize,
) -> Result<Option<PeerMessage>, Box<dyn std::error::Error + Send + Sync>> {
    read_framed_message(recv, max_size).await
}

/// Read raw bytes from the stream then deserialize with rkyv.
async fn read_framed_message<T>(
    recv: &mut iroh::endpoint::RecvStream,
    max_size: usize,
) -> Result<Option<T>, Box<dyn std::error::Error + Send + Sync>>
where
    T: rkyv::Archive,
    T::Archived: for<'a> rkyv::bytecheck::CheckBytes<rkyv::api::high::HighValidator<'a, rkyv::rancor::Error>>
        + rkyv::Deserialize<T, rkyv::rancor::Strategy<rkyv::de::Pool, rkyv::rancor::Error>>,
{
    // Read 4-byte length prefix.
    let mut len_buf = [0u8; 4];
    if let Err(e) = read_exact_chunked(recv, &mut len_buf).await {
        let msg = e.to_string();
        if msg.contains("closed") || msg.contains("finished") || msg.contains("reset") {
            return Ok(None);
        }
        return Err(format!("read len: {e}").into());
    }

    let len = u32::from_le_bytes(len_buf) as usize;
    if len > max_size {
        return Err(format!("message too large: {len} > {max_size}").into());
    }

    // Read payload using chunked reads.
    let mut buf = vec![0u8; len];
    read_exact_chunked(recv, &mut buf).await.map_err(|e| format!("read payload ({len} bytes): {e}"))?;

    let msg = rkyv::from_bytes::<T, rkyv::rancor::Error>(&buf).map_err(|e| format!("rkyv deserialize: {e}"))?;
    Ok(Some(msg))
}

/// Read exactly `buf.len()` bytes using chunked reads.
async fn read_exact_chunked(
    recv: &mut iroh::endpoint::RecvStream,
    buf: &mut [u8],
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let total = buf.len();
    let mut pos = 0;
    while pos < total {
        match recv.read(&mut buf[pos..]).await {
            Ok(Some(n)) => {
                if n == 0 {
                    return Err(format!("unexpected zero-length read at byte {pos}/{total}").into());
                }
                pos += n;
            }
            Ok(None) => {
                return Err(format!("stream ended at byte {pos}/{total}").into());
            }
            Err(e) => {
                return Err(format!("read error at byte {pos}/{total}: {e}").into());
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::collab::types::Annotation;

    // ─── CollabRenderOptions default ─────────────────────────────────────

    #[test]
    fn render_options_default_all_false() {
        let opts = CollabRenderOptions::default();
        assert!(!opts.show_hp_bars);
        assert!(!opts.show_tracers);
        assert!(!opts.show_torpedoes);
        assert!(!opts.show_planes);
        assert!(!opts.show_smoke);
        assert!(!opts.show_score);
        assert!(!opts.show_timer);
        assert!(!opts.show_kill_feed);
        assert!(!opts.show_player_names);
        assert!(!opts.show_ship_names);
        assert!(!opts.show_capture_points);
        assert!(!opts.show_buildings);
        assert!(!opts.show_turret_direction);
        assert!(!opts.show_consumables);
        assert!(!opts.show_dead_ships);
        assert!(!opts.show_dead_ship_names);
        assert!(!opts.show_armament);
        assert!(!opts.show_trails);
        assert!(!opts.show_dead_trails);
        assert!(!opts.show_speed_trails);
        assert!(!opts.show_battle_result);
        assert!(!opts.show_buffs);
        assert!(!opts.show_ship_config);
        assert!(!opts.show_chat);
        assert!(!opts.show_advantage);
        assert!(!opts.show_score_timer);
        assert!(!opts.show_self_detection_range);
        assert!(!opts.show_self_main_battery_range);
        assert!(!opts.show_self_secondary_range);
        assert!(!opts.show_self_torpedo_range);
        assert!(!opts.show_self_radar_range);
        assert!(!opts.show_self_hydro_range);
    }

    // ─── set_field ───────────────────────────────────────────────────────

    #[test]
    fn set_field_sets_correct_field() {
        let mut opts = CollabRenderOptions::default();
        opts.set_field(DisplayOptionField::ShowHpBars, true);
        assert!(opts.show_hp_bars);
        assert!(!opts.show_tracers);

        opts.set_field(DisplayOptionField::ShowSelfRadar, true);
        assert!(opts.show_self_radar_range);

        opts.set_field(DisplayOptionField::ShowHpBars, false);
        assert!(!opts.show_hp_bars);
    }

    #[test]
    fn set_field_all_variants() {
        // Verify every DisplayOptionField variant can be set and read back.
        let fields = [
            DisplayOptionField::ShowHpBars,
            DisplayOptionField::ShowTracers,
            DisplayOptionField::ShowTorpedoes,
            DisplayOptionField::ShowPlanes,
            DisplayOptionField::ShowSmoke,
            DisplayOptionField::ShowScore,
            DisplayOptionField::ShowTimer,
            DisplayOptionField::ShowKillFeed,
            DisplayOptionField::ShowPlayerNames,
            DisplayOptionField::ShowShipNames,
            DisplayOptionField::ShowCapturePoints,
            DisplayOptionField::ShowBuildings,
            DisplayOptionField::ShowTurretDirection,
            DisplayOptionField::ShowConsumables,
            DisplayOptionField::ShowDeadShips,
            DisplayOptionField::ShowDeadShipNames,
            DisplayOptionField::ShowArmament,
            DisplayOptionField::ShowTrails,
            DisplayOptionField::ShowDeadTrails,
            DisplayOptionField::ShowSpeedTrails,
            DisplayOptionField::ShowBattleResult,
            DisplayOptionField::ShowBuffs,
            DisplayOptionField::ShowShipConfig,
            DisplayOptionField::ShowChat,
            DisplayOptionField::ShowAdvantage,
            DisplayOptionField::ShowScoreTimer,
            DisplayOptionField::ShowSelfDetection,
            DisplayOptionField::ShowSelfMainBattery,
            DisplayOptionField::ShowSelfSecondary,
            DisplayOptionField::ShowSelfTorpedo,
            DisplayOptionField::ShowSelfRadar,
            DisplayOptionField::ShowSelfHydro,
        ];

        for field in fields {
            let mut opts = CollabRenderOptions::default();
            opts.set_field(field, true);
            // Verify exactly one field changed via diff.
            let diff = CollabRenderOptions::default().diff(&opts);
            assert_eq!(diff.len(), 1, "Expected exactly 1 diff for {:?}", field);
            assert_eq!(diff[0].0, field);
            assert!(diff[0].1);
        }
    }

    // ─── diff ────────────────────────────────────────────────────────────

    #[test]
    fn diff_identical_returns_empty() {
        let a = CollabRenderOptions::default();
        let b = CollabRenderOptions::default();
        assert!(a.diff(&b).is_empty());
    }

    #[test]
    fn diff_single_change() {
        let a = CollabRenderOptions::default();
        let mut b = CollabRenderOptions::default();
        b.show_tracers = true;
        let diff = a.diff(&b);
        assert_eq!(diff.len(), 1);
        assert_eq!(diff[0], (DisplayOptionField::ShowTracers, true));
    }

    #[test]
    fn diff_multiple_changes() {
        let a = CollabRenderOptions::default();
        let mut b = CollabRenderOptions::default();
        b.show_tracers = true;
        b.show_chat = true;
        b.show_self_hydro_range = true;
        let diff = a.diff(&b);
        assert_eq!(diff.len(), 3);
        let fields: Vec<_> = diff.iter().map(|(f, _)| *f).collect();
        assert!(fields.contains(&DisplayOptionField::ShowTracers));
        assert!(fields.contains(&DisplayOptionField::ShowChat));
        assert!(fields.contains(&DisplayOptionField::ShowSelfHydro));
    }

    #[test]
    fn diff_detects_false_to_true_and_true_to_false() {
        let mut a = CollabRenderOptions::default();
        a.show_hp_bars = true;
        let b = CollabRenderOptions::default();
        // a has show_hp_bars=true, b has false
        let diff = a.diff(&b);
        assert_eq!(diff.len(), 1);
        assert_eq!(diff[0], (DisplayOptionField::ShowHpBars, false));
    }

    // ─── Message framing ─────────────────────────────────────────────────

    /// Copy payload into an AlignedVec for rkyv deserialization (mirrors production read path).
    fn deserialize_payload(framed: &[u8]) -> PeerMessage {
        let len = u32::from_le_bytes(framed[0..4].try_into().unwrap()) as usize;
        assert_eq!(len, framed.len() - 4);
        let payload = &framed[4..];
        // Copy into an AlignedVec so rkyv has proper alignment.
        let mut aligned = rkyv::util::AlignedVec::<16>::new();
        aligned.extend_from_slice(payload);
        deserialize_message(&aligned).unwrap()
    }

    #[test]
    fn frame_peer_message_roundtrip() {
        let msg = PeerMessage::CursorPosition(Some([100.0, 200.0]));
        let framed = frame_peer_message(&msg).unwrap();

        // Should start with a 4-byte LE length prefix.
        assert!(framed.len() > 4);

        let decoded = deserialize_payload(&framed);
        match decoded {
            PeerMessage::CursorPosition(Some(pos)) => {
                assert!((pos[0] - 100.0).abs() < f32::EPSILON);
                assert!((pos[1] - 200.0).abs() < f32::EPSILON);
            }
            _ => panic!("Expected CursorPosition(Some(...))"),
        }
    }

    #[test]
    fn frame_peer_message_various_types() {
        // Verify several message types can be framed without error.
        let messages = vec![
            PeerMessage::Join {
                name: "Test".into(),
                client_type: ClientType::Desktop { toolkit_version: "1.0".into() },
            },
            PeerMessage::CursorPosition(None),
            PeerMessage::ClearAnnotations { board_id: None },
            PeerMessage::Permissions { annotations_locked: true, settings_locked: false },
            PeerMessage::UserLeft { user_id: 42 },
            PeerMessage::PromoteToCoHost { user_id: 3 },
            PeerMessage::Ping { pos: [100.0, 200.0] },
            PeerMessage::RemoveAnnotation { board_id: None, id: 999 },
            PeerMessage::PlaybackState { playing: true, speed: 1.5 },
            PeerMessage::Heartbeat,
        ];
        for msg in &messages {
            assert!(frame_peer_message(msg).is_ok(), "Failed to frame: {:?}", msg);
        }
    }

    #[test]
    fn frame_annotation_sync_roundtrip() {
        let msg = PeerMessage::AnnotationSync {
            board_id: None,
            annotations: vec![Annotation::Circle {
                center: [100.0, 200.0],
                radius: 50.0,
                color: [255, 0, 0, 128],
                width: 3.0,
                filled: true,
            }],
            owners: vec![0],
            ids: vec![42],
        };
        let framed = frame_peer_message(&msg).unwrap();
        let decoded = deserialize_payload(&framed);
        match decoded {
            PeerMessage::AnnotationSync { annotations, owners, ids, .. } => {
                assert_eq!(annotations.len(), 1);
                assert_eq!(owners, vec![0]);
                assert_eq!(ids, vec![42]);
            }
            _ => panic!("Expected AnnotationSync"),
        }
    }

    // ─── Constants sanity ────────────────────────────────────────────────

    #[test]
    fn constants_are_reasonable() {
        assert!(MAX_MESSAGE_SIZE > MAX_FRAME_SIZE);
        assert!(MAX_DECOMPRESSED_FRAME_SIZE > MAX_FRAME_SIZE);
        assert!(MAX_DISPLAY_NAME_LEN <= MAX_STRING_LEN);
        assert!(COORD_MIN < COORD_MAX);
        assert!(MAX_PEERS >= 2);
        assert!(MAX_ANNOTATIONS > 0);
        assert!(MAX_FREEHAND_POINTS > 0);
    }
}

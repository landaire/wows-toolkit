use kinded::Kinded;
use winnow::Parser as _;
use winnow::binary::le_f32;
use winnow::binary::le_i16;
use winnow::binary::le_i32;
use winnow::binary::le_i64;
use winnow::binary::le_u8;
use winnow::binary::le_u16;
use winnow::binary::le_u32;
use winnow::token::take;

use serde::Serialize;
use std::cell::RefCell;
use std::collections::HashMap;
use std::convert::TryInto;

use crate::error::*;
use crate::types::EntityId;
use crate::types::GameClock;
use crate::types::GameParamId;
use crate::types::WeaponLockType;
use crate::types::WeaponType;
use wowsunpack::data::Version;
use wowsunpack::recognized::Recognized;
use wowsunpack::rpc::entitydefs::*;
use wowsunpack::rpc::typedefs::ArgValue;

/// A non-fatal parsing observation: a method/property payload was not fully
/// consumed, which usually signals a def/layout mismatch for this build.
#[derive(Clone, Debug, PartialEq)]
pub struct PayloadDiagnostic {
    /// Human-readable origin, e.g. "EntityMethod::Avatar::onFoo".
    pub context: String,
    /// The def semantic name of the value being parsed, when known.
    pub semantic_name: Option<String>,
    pub payload_len: usize,
    pub consumed: usize,
}

impl PayloadDiagnostic {
    /// Some(_) only when the payload was under-consumed.
    pub fn for_leftover(
        context: String,
        semantic_name: Option<String>,
        payload_len: usize,
        consumed: usize,
    ) -> Option<Self> {
        if consumed < payload_len { Some(Self { context, semantic_name, payload_len, consumed }) } else { None }
    }
}

#[derive(Debug, Serialize, Clone)]
pub struct Vec3 {
    pub x: f32,
    pub y: f32,
    pub z: f32,
}

pub fn parse_vec3(i: &mut &[u8]) -> PResult<Vec3> {
    let x = le_f32.parse_next(i)?;
    let y = le_f32.parse_next(i)?;
    let z = le_f32.parse_next(i)?;
    Ok(Vec3 { x, y, z })
}

#[derive(Debug, Serialize, Clone)]
pub struct Rot3 {
    pub yaw: f32,
    pub pitch: f32,
    pub roll: f32,
}

pub fn parse_rot3(i: &mut &[u8]) -> PResult<Rot3> {
    let yaw = le_f32.parse_next(i)?;
    let pitch = le_f32.parse_next(i)?;
    let roll = le_f32.parse_next(i)?;
    Ok(Rot3 { roll, pitch, yaw })
}

#[derive(Debug, Serialize, Clone)]
pub struct PositionPacket {
    pub pid: EntityId,
    /// Space ID (always 0 in observed replays).
    pub space_id: u32,
    pub position: Vec3,
    /// Direction/velocity vector for entity movement interpolation (dead reckoning).
    /// Zero when written by updateOfflinePositions, non-zero from live position updates.
    pub direction: Vec3,
    pub rotation: Rot3,
    /// On-ground flag (usually 1 for ships).
    pub is_on_ground: bool,
}

#[derive(Debug, Serialize)]
pub struct EntityPacket<'replay> {
    pub supertype: u32,
    pub entity_id: EntityId,
    pub subtype: u32,
    pub payload: &'replay [u8],
}

#[derive(Debug, Serialize)]
pub struct EntityPropertyPacket<'argtype> {
    pub entity_id: EntityId,
    pub property: &'argtype str,
    pub value: ArgValue<'argtype>,
}

#[derive(Debug, Serialize)]
pub struct EntityMethodPacket<'argtype> {
    pub entity_id: EntityId,
    pub method: &'argtype str,
    pub args: Vec<ArgValue<'argtype>>,
}

#[derive(Debug, Serialize)]
pub struct EntityCreatePacket<'argtype> {
    pub entity_id: EntityId,
    pub spec_idx: usize,
    pub entity_type: &'argtype str,
    pub space_id: u32,
    pub vehicle_id: GameParamId,
    pub position: Vec3,
    pub rotation: Rot3,
    pub state_length: u32,
    pub props: HashMap<&'argtype str, ArgValue<'argtype>>,
}

/// Note that this packet frequently appears twice - it appears that it
/// describes both the player's boat location/orientation as well as the
/// camera orientation. When the camera is attached to an object, the ID of
/// that object will be given in the parent_id field.
#[derive(Debug, Serialize, Clone)]
pub struct PlayerOrientationPacket {
    pub pid: EntityId,
    pub parent_id: EntityId,
    pub position: Vec3,
    pub rotation: Rot3,
}

#[derive(Debug, Serialize)]
pub struct InvalidPacket<'a> {
    message: String,
    raw: &'a [u8],
}

#[derive(Debug, Serialize)]
pub struct BasePlayerCreatePacket<'argtype> {
    pub entity_id: EntityId,
    pub entity_type: &'argtype str,
    pub props: HashMap<&'argtype str, ArgValue<'argtype>>,
    /// Trailing data after base properties (likely BigWorld component state)
    #[serde(skip_serializing_if = "<[u8]>::is_empty")]
    pub component_data: Vec<u8>,
}

#[derive(Debug, Serialize)]
pub struct CellPlayerCreatePacket<'argtype> {
    pub entity_id: EntityId,
    pub entity_type: &'argtype str,
    pub space_id: u32,
    pub vehicle_id: GameParamId,
    pub position: Vec3,
    pub rotation: Rot3,
    pub props: HashMap<&'argtype str, ArgValue<'argtype>>,
    /// Trailing data after internal properties (likely BigWorld component state)
    #[serde(skip_serializing_if = "<[u8]>::is_empty")]
    pub component_data: Vec<u8>,
}

#[derive(Debug, Serialize)]
pub struct EntityLeavePacket {
    pub entity_id: EntityId,
}

#[derive(Debug, Serialize)]
pub struct EntityEnterPacket {
    pub entity_id: EntityId,
    pub space_id: u32,
    pub vehicle_id: GameParamId,
}

#[derive(Debug, Serialize)]
pub struct PropertyUpdatePacket<'argtype> {
    /// Indicates the entity to update the property on
    pub entity_id: EntityId,
    /// Indicates the property to update. Note that some properties have many
    /// sub-properties.
    pub property: &'argtype str,
    /// Indicates the update command to perform.
    pub update_cmd: crate::nested_property_path::PropertyNesting<'argtype>,
}

/// Packet 0x25: Camera state. Written every tick alongside GunMarker (0x18)
/// and PlayerNetStats (0x1d). 60 bytes total.
#[derive(Debug, Serialize, Clone)]
pub struct CameraPacket {
    /// Camera rotation quaternion (x, y, z, w).
    pub rotation_quat: [f32; 4],
    /// Camera position in world space.
    pub camera_position: Vec3,
    /// Field of view in radians.
    pub fov: f32,
    /// Unknown float (observed as 1.0). Set by packet 0x15, paired with
    /// spaceID from packet 0x14. Initialized to -1.0 sentinel.
    pub unknown: f32,
    /// Player/entity position in world space.
    pub position: Vec3,
    /// Player/entity direction in world space.
    pub direction: Vec3,
}

#[derive(Debug, Serialize)]
pub struct CruiseState {
    pub key: u32,
    pub value: i32,
}

/// Packet 0x02: EntityControl — transfers entity ownership to the client.
/// Confirmed by the Python reference parser's PACKETS_MAPPING.
#[derive(Debug, Serialize)]
pub struct EntityControlPacket {
    pub entity_id: EntityId,
    pub is_controlled: bool,
}

/// Packet 0x2a: Non-volatile entity position update. BigWorld
/// `avatarUpdateNoAliasFullPosYawPitchRoll` — same format as Position
/// packet but without direction vector and is_on_ground. Dispatched via
/// `BWEntitiesListener::handleEntityNonVolatileMove`. Used for entities
/// that don't need dead-reckoning interpolation (SmokeScreen, weather zones).
#[derive(Debug, Serialize)]
pub struct NonVolatilePositionPacket {
    pub entity_id: EntityId,
    /// Space ID (always 0 in observed replays).
    pub space_id: u32,
    /// Updated world-space position of the smoke cloud.
    pub position: Vec3,
    /// Updated rotation (yaw/pitch/roll).
    pub rotation: Rot3,
}

/// Packet 0x1d: Player network stats. Written every tick alongside GunMarker
/// (0x18) and Camera (0x25) packets. Packs three values into a single u32:
/// - Bits 0-7: fps (clamped to 0-255, stored as `replayFps` on reader side)
/// - Bits 8-23: ping in ms (clamped to 0-999, stored as `replayPing` on reader side)
/// - Bit 24: isLaggingNow (bool)
///   Not emitted when fps == -1 (uninitialized sentinel).
mod raw_player_net_stats {
    #![allow(dead_code)]
    use modular_bitfield::prelude::*;

    #[bitfield]
    #[derive(Debug)]
    pub(crate) struct RawPlayerNetStats {
        pub fps: B8,
        pub ping: B16,
        pub is_lagging: bool,
        #[skip]
        _unused: B7,
    }
}
use raw_player_net_stats::RawPlayerNetStats;

#[derive(Debug, Serialize, Clone)]
pub struct PlayerNetStatsPacket {
    pub fps: u8,
    pub ping: u16,
    pub is_lagging: bool,
}

impl PlayerNetStatsPacket {
    /// Decode a PlayerNetStats (0x1d) payload: a single packed u32. Spec-free,
    /// so it works on a [`RawPacket`] without a full [`Parser`].
    pub fn from_payload(payload: &[u8]) -> Option<Self> {
        let bytes: [u8; 4] = payload.get(..4)?.try_into().ok()?;
        let stats = RawPlayerNetStats::from_bytes(bytes);
        Some(Self { fps: stats.fps(), ping: stats.ping(), is_lagging: stats.is_lagging() })
    }
}

/// Packet 0x0f: Server timestamp. Single f64 at clock=0.
#[derive(Debug, Serialize)]
pub struct ServerTimestampPacket {
    pub timestamp: f64,
}

/// Packet 0x20: Links the Avatar to its owned ship entity. The entity ID
/// matches the Avatar's `ownShipId` property.
#[derive(Debug, Serialize)]
pub struct OwnShipPacket {
    pub entity_id: EntityId,
}

/// Packet 0x30: `onSetWeaponLock` — weapon lock state change. The cell broadcasts
/// it to clients; `WGReplayController::_py_onSetWeaponLock` records the call.
///
/// Wire layout is three `u32`s: `(weaponType, lockType, targetId)`. The Python
/// client's lock API uses this same argument order throughout
/// (`setWeaponLock(weaponType, lockType, targetId, ...)`). Note the recorded
/// broadcast carries no aim point — the trailing `tarPos` exists only on the
/// upstream client->cell `setWeaponLock` call, not here.
#[derive(Debug, Serialize, Clone)]
pub struct SetWeaponLockPacket {
    /// Which weapon system this lock applies to.
    pub weapon_type: Recognized<WeaponType, u32>,
    /// The lock mode (none / absolute point / relative point / target).
    pub lock_type: Recognized<WeaponLockType, u32>,
    /// Entity ID of the lock target. Meaningful only when `lock_type` is
    /// `Target`; otherwise typically 0.
    pub target_id: EntityId,
}

/// Packet 0x31: Submarine controller mode change. Only present in submarine replays.
/// Recorded by `WGReplayController::onChangeSubController(i16 mode)`.
/// The `mode` value toggles between 0 and 1 (likely surface/dive states).
#[derive(Debug, Serialize, Clone)]
pub struct SubControllerPacket {
    pub mode: i16,
}

/// Packet 0x33: Shot tracking change. Present in replays from ~Feb 2026 onward.
/// Recorded by `WGReplayController::onChangeShotTracking(i32 entity_id, i64 value)`.
/// The entity_id is always the player's own vehicle. The value field's exact
/// semantics are unclear — it increases for battleships but decreases in bursts
/// for destroyers, possibly related to the fire control/target tracking system.
#[derive(Debug, Serialize, Clone)]
pub struct ShotTrackingPacket {
    pub entity_id: EntityId,
    pub value: i64,
}

/// Packet 0x18: Gun marker / aiming state. Written every tick alongside
/// Camera (0x25) and PlayerNetStats (0x1d) packets. The C++ replay controller
/// has fields for gun rotator target point, gun marker position/direction/
/// diameter, arcade marker size, and SPG marker params. However, in modern
/// game versions (at least since ~13.x) the Python code never sets these
/// properties — gun marker logic was moved to `GunMarkerSystem` which
/// bypasses the replay controller. As a result, these fields are always
/// zero/default in practice. The game's reader only consumes the first
/// 16 bytes (target point + diameter) during playback.
#[derive(Debug, Serialize, Clone)]
pub struct GunMarkerPacket {
    /// World-space point the gun rotator is aiming at.
    pub target_point: Vec3,
    /// Gun marker diameter (dispersion circle size).
    pub diameter: f32,
    /// Gun marker world-space position.
    pub marker_position: Vec3,
    /// Gun marker direction vector.
    pub marker_direction: Vec3,
    /// Arcade gun marker size. Defaults to -1.0 when unset.
    pub arcade_marker_size: f32,
    /// SPG (artillery view) gun marker params: two floats. Default to -1.0 when unset.
    pub spg_marker_params: (f32, f32),
}

#[derive(Debug, Serialize)]
pub struct MapPacket<'replay> {
    pub space_id: u32,
    pub arena_id: i64,
    pub unknown1: u32,
    pub unknown2: u32,
    pub blob: &'replay [u8],
    pub map_name: &'replay str,
    /// Note: We suspect that this matrix is always the unit matrix, hence
    /// we don't spend the computation to parse it.
    pub matrix: &'replay [u8],
    pub unknown: u8, // bool?
}

#[derive(Debug, Serialize, Kinded)]
pub enum PacketType<'replay, 'argtype> {
    Position(PositionPacket),
    BasePlayerCreate(BasePlayerCreatePacket<'argtype>),
    CellPlayerCreate(CellPlayerCreatePacket<'argtype>),
    EntityEnter(EntityEnterPacket),
    EntityLeave(EntityLeavePacket),
    EntityCreate(EntityCreatePacket<'argtype>),
    EntityProperty(EntityPropertyPacket<'argtype>),
    EntityMethod(EntityMethodPacket<'argtype>),
    PropertyUpdate(PropertyUpdatePacket<'argtype>),
    PlayerOrientation(PlayerOrientationPacket),
    CruiseState(CruiseState),
    Version(String),
    Camera(CameraPacket),
    CameraMode(u32),
    CameraFreeLook(u8),
    Map(MapPacket<'replay>),
    BattleResults(&'replay str),
    EntityControl(EntityControlPacket),
    NonVolatilePosition(NonVolatilePositionPacket),
    PlayerNetStats(PlayerNetStatsPacket),
    ServerTimestamp(ServerTimestampPacket),
    OwnShip(OwnShipPacket),
    SetWeaponLock(SetWeaponLockPacket),
    /// Packet 0x0e: Server tick rate constant (always 1/7).
    ServerTick(f64),
    SubController(SubControllerPacket),
    ShotTracking(ShotTrackingPacket),
    GunMarker(GunMarkerPacket),
    /// Packet 0x10: Init flag sent at clock=0. Always 0.
    InitFlag(u8),
    /// Packet 0x13: Empty init marker at clock=0.
    InitMarker,
    Unknown(&'replay [u8]),

    /// These are packets which we thought we understood, but couldn't parse
    Invalid(InvalidPacket<'replay>),
}

#[derive(Debug, Serialize)]
pub struct Packet<'replay, 'argtype> {
    pub packet_size: u32,
    pub packet_type: PacketTypeId,
    pub clock: GameClock,
    pub payload: PacketType<'replay, 'argtype>,
    pub raw: &'replay [u8],
    /// Bytes remaining after the parser consumed data from the packet payload.
    /// Non-empty means the parser didn't consume the full packet.
    #[serde(skip_serializing_if = "<[u8]>::is_empty")]
    pub leftover: &'replay [u8],
}

/// Packet type identifier from the BigWorld replay protocol.
///
/// Fail-open: any unrecognised wire value becomes [`PacketTypeId::Unknown`]
/// carrying the raw `u32`, so a new game version never breaks the walker. The
/// `Debug` impl prints the hex wire identifier (e.g. `EntityMethod(0x08)`).
#[derive(Clone, Copy, PartialEq, Eq, Hash)]
pub enum PacketTypeId {
    BasePlayerCreate,
    BasePlayerCreateStub,
    CellPlayerCreate,
    EntityControl,
    EntityEnter,
    EntityLeave,
    EntityCreate,
    EntityProperty,
    EntityMethod,
    Position,
    ServerTick,
    ServerTimestamp,
    InitFlag,
    InitMarker,
    Version,
    GunMarker,
    PlayerNetStats,
    OwnShip,
    BattleResults,
    NestedPropertyUpdate,
    Camera,
    CameraMode,
    Map,
    NonVolatilePosition,
    PlayerOrientation,
    CameraFreeLook,
    SetWeaponLock,
    SubController,
    CruiseState,
    ShotTracking,
    Unknown(u32),
}

/// Game version (major, minor, patch) at which the modern BigWorld packet-ID
/// layout begins: `BattleResults` occupies `0x22` and every packet at `0x23`
/// and beyond shifts up one slot from the legacy layout.
///
/// Determined empirically from per-version raw packet-ID histograms (the
/// 32-byte PlayerOrientation and 60-byte Camera packets pin the layout): 12.3.1,
/// 12.4.0 and 12.5.0 use the legacy layout (PlayerOrientation `0x2b`, Camera
/// `0x24`); 12.6.0 is the first modern build (PlayerOrientation `0x2c`, Camera
/// `0x25`). Gating on the semantic version rather than the raw build number is
/// deliberate: regional clients share a version but have different build numbers.
pub const MODERN_PACKET_LAYOUT_MIN_VERSION: (u32, u32, u32) = (12, 6, 0);

impl PacketTypeId {
    /// Map a raw wire identifier using the modern layout. Unrecognised values
    /// become `Unknown(raw)`. Prefer [`Self::from_raw_for_version`] when the
    /// replay's version is known so older replays dispatch correctly across the
    /// layout shift.
    pub fn from_raw(raw: u32) -> Self {
        Self::from_raw_modern(raw)
    }

    /// Version-keyed packet-ID mapping. Before
    /// [`MODERN_PACKET_LAYOUT_MIN_VERSION`] the wire layout had no `BattleResults`
    /// packet and ran each subsequent ID one slot lower (0x22 was
    /// NestedPropertyUpdate, 0x24 Camera, 0x27 Map, 0x2b PlayerOrientation, etc.).
    pub fn from_raw_for_version(raw: u32, version: &Version) -> Self {
        let v = (version.major, version.minor, version.patch);
        if v >= MODERN_PACKET_LAYOUT_MIN_VERSION { Self::from_raw_modern(raw) } else { Self::from_raw_legacy(raw) }
    }

    fn from_raw_modern(raw: u32) -> Self {
        match raw {
            0x00 => Self::BasePlayerCreate,
            0x26 => Self::BasePlayerCreateStub,
            0x01 => Self::CellPlayerCreate,
            0x02 => Self::EntityControl,
            0x03 => Self::EntityEnter,
            0x04 => Self::EntityLeave,
            0x05 => Self::EntityCreate,
            0x07 => Self::EntityProperty,
            0x08 => Self::EntityMethod,
            0x0a => Self::Position,
            0x0e => Self::ServerTick,
            0x0f => Self::ServerTimestamp,
            0x10 => Self::InitFlag,
            0x13 => Self::InitMarker,
            0x16 => Self::Version,
            0x18 => Self::GunMarker,
            0x1d => Self::PlayerNetStats,
            0x20 => Self::OwnShip,
            0x22 => Self::BattleResults,
            0x23 => Self::NestedPropertyUpdate,
            0x25 => Self::Camera,
            0x27 => Self::CameraMode,
            0x28 => Self::Map,
            0x2a => Self::NonVolatilePosition,
            0x2c => Self::PlayerOrientation,
            0x2f => Self::CameraFreeLook,
            0x30 => Self::SetWeaponLock,
            0x31 => Self::SubController,
            0x32 => Self::CruiseState,
            0x33 => Self::ShotTracking,
            other => Self::Unknown(other),
        }
    }

    fn from_raw_legacy(raw: u32) -> Self {
        match raw {
            0x00 => Self::BasePlayerCreate,
            0x26 => Self::BasePlayerCreateStub,
            0x01 => Self::CellPlayerCreate,
            0x02 => Self::EntityControl,
            0x03 => Self::EntityEnter,
            0x04 => Self::EntityLeave,
            0x05 => Self::EntityCreate,
            0x07 => Self::EntityProperty,
            0x08 => Self::EntityMethod,
            0x0a => Self::Position,
            0x0e => Self::ServerTick,
            0x0f => Self::ServerTimestamp,
            0x10 => Self::InitFlag,
            0x13 => Self::InitMarker,
            0x16 => Self::Version,
            0x18 => Self::GunMarker,
            0x1d => Self::PlayerNetStats,
            0x20 => Self::OwnShip,
            0x22 => Self::NestedPropertyUpdate,
            0x24 => Self::Camera,
            0x27 => Self::Map,
            0x29 => Self::NonVolatilePosition,
            0x2b => Self::PlayerOrientation,
            0x2e => Self::CameraFreeLook,
            0x2f => Self::SetWeaponLock,
            0x30 => Self::SubController,
            0x31 => Self::CruiseState,
            0x32 => Self::ShotTracking,
            other => Self::Unknown(other),
        }
    }

    /// The raw wire identifier.
    pub fn raw(self) -> u32 {
        match self {
            Self::BasePlayerCreate => 0x00,
            Self::BasePlayerCreateStub => 0x26,
            Self::CellPlayerCreate => 0x01,
            Self::EntityControl => 0x02,
            Self::EntityEnter => 0x03,
            Self::EntityLeave => 0x04,
            Self::EntityCreate => 0x05,
            Self::EntityProperty => 0x07,
            Self::EntityMethod => 0x08,
            Self::Position => 0x0a,
            Self::ServerTick => 0x0e,
            Self::ServerTimestamp => 0x0f,
            Self::InitFlag => 0x10,
            Self::InitMarker => 0x13,
            Self::Version => 0x16,
            Self::GunMarker => 0x18,
            Self::PlayerNetStats => 0x1d,
            Self::OwnShip => 0x20,
            Self::BattleResults => 0x22,
            Self::NestedPropertyUpdate => 0x23,
            Self::Camera => 0x25,
            Self::CameraMode => 0x27,
            Self::Map => 0x28,
            Self::NonVolatilePosition => 0x2a,
            Self::PlayerOrientation => 0x2c,
            Self::CameraFreeLook => 0x2f,
            Self::SetWeaponLock => 0x30,
            Self::SubController => 0x31,
            Self::CruiseState => 0x32,
            Self::ShotTracking => 0x33,
            Self::Unknown(raw) => raw,
        }
    }

    fn name(self) -> &'static str {
        match self {
            Self::BasePlayerCreate => "BasePlayerCreate",
            Self::BasePlayerCreateStub => "BasePlayerCreateStub",
            Self::CellPlayerCreate => "CellPlayerCreate",
            Self::EntityControl => "EntityControl",
            Self::EntityEnter => "EntityEnter",
            Self::EntityLeave => "EntityLeave",
            Self::EntityCreate => "EntityCreate",
            Self::EntityProperty => "EntityProperty",
            Self::EntityMethod => "EntityMethod",
            Self::Position => "Position",
            Self::ServerTick => "ServerTick",
            Self::ServerTimestamp => "ServerTimestamp",
            Self::InitFlag => "InitFlag",
            Self::InitMarker => "InitMarker",
            Self::Version => "Version",
            Self::GunMarker => "GunMarker",
            Self::PlayerNetStats => "PlayerNetStats",
            Self::OwnShip => "OwnShip",
            Self::BattleResults => "BattleResults",
            Self::NestedPropertyUpdate => "NestedPropertyUpdate",
            Self::Camera => "Camera",
            Self::CameraMode => "CameraMode",
            Self::Map => "Map",
            Self::NonVolatilePosition => "NonVolatilePosition",
            Self::PlayerOrientation => "PlayerOrientation",
            Self::CameraFreeLook => "CameraFreeLook",
            Self::SetWeaponLock => "SetWeaponLock",
            Self::SubController => "SubController",
            Self::CruiseState => "CruiseState",
            Self::ShotTracking => "ShotTracking",
            Self::Unknown(_) => "Unknown",
        }
    }
}

impl std::fmt::Debug for PacketTypeId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}(0x{:02x})", self.name(), self.raw())
    }
}

impl serde::Serialize for PacketTypeId {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_u32(self.raw())
    }
}

/// A packet with its 12-byte header parsed but the payload left raw.
///
/// Produced by [`RawPacketIterator`]. Lets callers walk a decrypted replay
/// stream without entity definitions: the header is always available, and
/// spec-independent payloads can be decoded on demand (e.g.
/// [`PlayerNetStatsPacket::from_payload`]).
#[derive(Debug, Clone, Copy)]
pub struct RawPacket<'a> {
    pub packet_size: u32,
    pub packet_type: PacketTypeId,
    pub clock: GameClock,
    pub payload: &'a [u8],
}

/// Sans-io iterator over a decrypted packet stream. Parses only packet headers,
/// never the entity-spec-dependent payloads, so it needs no entity definitions
/// and is safe in any environment (wasm, embedded). On a malformed/truncated
/// stream it yields one `Err` and then stops.
pub struct RawPacketIterator<'a> {
    remaining: &'a [u8],
    version: Option<Version>,
}

impl<'a> RawPacketIterator<'a> {
    /// Walk a packet stream assuming the modern packet-ID layout. For replays
    /// from older versions, use [`Self::with_version`] so packet IDs dispatch
    /// correctly across the layout shift at
    /// [`MODERN_PACKET_LAYOUT_MIN_VERSION`].
    pub fn new(packet_data: &'a [u8]) -> Self {
        Self { remaining: packet_data, version: None }
    }

    /// Walk a packet stream using the packet-ID layout that matches `version`.
    pub fn with_version(packet_data: &'a [u8], version: Version) -> Self {
        Self { remaining: packet_data, version: Some(version) }
    }
}

impl<'a> Iterator for RawPacketIterator<'a> {
    type Item = PResult<RawPacket<'a>>;

    fn next(&mut self) -> Option<Self::Item> {
        if self.remaining.is_empty() {
            return None;
        }
        let version = self.version;
        let result = (|| {
            let packet_size = le_u32.parse_next(&mut self.remaining)?;
            let raw_type = le_u32.parse_next(&mut self.remaining)?;
            let packet_type = match &version {
                Some(v) => PacketTypeId::from_raw_for_version(raw_type, v),
                None => PacketTypeId::from_raw(raw_type),
            };
            let raw_clock = le_f32.parse_next(&mut self.remaining)?;
            let payload = take(packet_size as usize).parse_next(&mut self.remaining)?;
            Ok(RawPacket { packet_size, packet_type, clock: GameClock(raw_clock), payload })
        })();
        if result.is_err() {
            self.remaining = &[];
        }
        Some(result)
    }
}

#[derive(Debug)]
pub struct Entity<'argtype> {
    entity_type: u16,
    properties: Vec<ArgValue<'argtype>>,
}

pub struct Parser<'argtype> {
    specs: &'argtype [EntitySpec],
    entities: HashMap<u32, Entity<'argtype>>,
    /// The replay's game version, used to select the packet-ID layout. `None`
    /// assumes the modern layout (when the version isn't known).
    version: Option<Version>,
    /// Interior mutability: parse methods borrow `self.specs` via shared refs,
    /// so `&mut self` accumulation would conflict with those borrows.
    diagnostics: RefCell<Vec<PayloadDiagnostic>>,
}

impl<'argtype> Parser<'argtype> {
    /// Build a parser assuming the modern packet-ID layout. Prefer
    /// [`Self::with_version`] when the replay's version is known so old replays
    /// dispatch correctly across the protocol shift at 12.6.0.
    pub fn new(entities: &'argtype [EntitySpec]) -> Parser<'argtype> {
        Parser { specs: entities, entities: HashMap::new(), version: None, diagnostics: RefCell::new(Vec::new()) }
    }

    /// Build a parser that dispatches packet IDs for the given game version.
    pub fn with_version(entities: &'argtype [EntitySpec], version: Version) -> Parser<'argtype> {
        Parser {
            specs: entities,
            entities: HashMap::new(),
            version: Some(version),
            diagnostics: RefCell::new(Vec::new()),
        }
    }

    /// Take and clear accumulated non-fatal diagnostics.
    pub fn drain_diagnostics(&self) -> Vec<PayloadDiagnostic> {
        self.diagnostics.borrow_mut().drain(..).collect()
    }

    fn parse_entity_property_packet<'a, 'b>(&'b self, i: &mut &'a [u8]) -> PResult<PacketType<'a, 'argtype>> {
        let entity_id = le_u32.parse_next(i)?;
        let prop_id = le_u32.parse_next(i)?;
        let payload_length = le_u32.parse_next(i)?;
        let payload: &'a [u8] = take(payload_length as usize).parse_next(i)?;

        // Return structured errors rather than panicking. The caller's
        // parse_packet wraps any Err into an Invalid packet, but the error
        // itself stays introspectable for direct callers.
        let entity = self.entities.get(&entity_id).ok_or_else(|| failure(ParseError::UnknownEntity { entity_id }))?;
        let entity_type = entity.entity_type;
        let spec_entity = (entity_type as usize)
            .checked_sub(1)
            .and_then(|idx| self.specs.get(idx))
            .ok_or_else(|| failure(ParseError::EntityTypeOutOfBounds { entity_type, spec_count: self.specs.len() }))?;
        let spec = spec_entity
            .properties
            .get(prop_id as usize)
            .ok_or_else(|| failure(ParseError::PropertyIdOutOfBounds { prop_id, entity_type }))?;

        let mut pslice = payload;
        let pval = spec.prop_type.parse_value(&mut pslice).map_err(|e| {
            failure(ParseError::RpcValueParseFailed {
                method: format!("EntityProperty::{}", spec.name),
                argnum: prop_id as usize,
                argtype: format!("{:?}", spec.prop_type),
                semantic_name: spec.prop_type.semantic_name().map(str::to_string),
                error: format!("{e:?}"),
            })
        })?;

        let consumed = payload.len() - pslice.len();
        if let Some(d) = PayloadDiagnostic::for_leftover(
            format!("EntityProperty::{}::{}", spec_entity.name, spec.name),
            spec.prop_type.semantic_name().map(str::to_string),
            payload.len(),
            consumed,
        ) {
            self.diagnostics.borrow_mut().push(d);
        }

        Ok(PacketType::EntityProperty(EntityPropertyPacket {
            entity_id: entity_id.into(),
            property: &spec.name,
            value: pval,
        }))
    }

    fn parse_entity_method_packet<'a, 'b>(&'b self, i: &mut &'a [u8]) -> PResult<PacketType<'a, 'b>> {
        let entity_id = le_u32.parse_next(i)?;
        let method_id = le_u32.parse_next(i)?;
        let payload_length = le_u32.parse_next(i)?;
        let payload: &'a [u8] = take(payload_length as usize).parse_next(i)?;
        if !i.is_empty() {
            return Err(failure(ParseError::InvalidPacketData));
        }

        // Return structured errors rather than panicking on a stream that
        // references an entity before its creation packet.
        let entity = self.entities.get(&entity_id).ok_or_else(|| failure(ParseError::UnknownEntity { entity_id }))?;
        let entity_type = entity.entity_type;

        let spec_entity = (entity_type as usize)
            .checked_sub(1)
            .and_then(|idx| self.specs.get(idx))
            .ok_or_else(|| failure(ParseError::EntityTypeOutOfBounds { entity_type, spec_count: self.specs.len() }))?;
        let methods = &spec_entity.client_methods;
        let spec = methods
            .get(method_id as usize)
            .ok_or_else(|| failure(ParseError::MethodIdOutOfBounds { method_id, entity_type }))?;

        // Fast-fail on exposed-method id drift. When every arg is fixed-size the
        // client always sends at least that many bytes, so a shorter payload means
        // this method_id resolved to the wrong spec: the exposed-method ordering no
        // longer matches the client, which happens when a type's on-wire size is
        // misclassified (e.g. a variable-length USER_TYPE sized as its fixed inner
        // type). sort_size() reports INFINITY (0xffff) for any variable arg, so a
        // sum below that means the method is wholly fixed-size. Over-long payloads
        // are legitimate (methods can carry trailing data) and are not flagged.
        let fixed_size: usize = spec.args.iter().map(|a| a.sort_size()).sum();
        if fixed_size < 0xffff && payload.len() < fixed_size {
            return Err(failure(ParseError::ExposedMethodMappingDrift {
                method: spec.name.to_string(),
                method_id,
                entity_type,
                expected: fixed_size,
                got: payload.len(),
            }));
        }
        let mut sub = payload;
        let mut args = vec![];
        for (idx, arg) in spec.args.iter().enumerate() {
            let pval = match arg.parse_value(&mut sub) {
                Ok(x) => x,
                Err(e) => {
                    return Err(failure(ParseError::RpcValueParseFailed {
                        method: spec.name.to_string(),
                        argnum: idx,
                        argtype: format!("{:?}", arg),
                        semantic_name: arg.semantic_name().map(str::to_string),
                        error: format!("{:?}", e),
                    }));
                }
            };
            args.push(pval);
        }

        Ok(PacketType::EntityMethod(EntityMethodPacket { entity_id: entity_id.into(), method: &spec.name, args }))
    }

    fn parse_battle_results<'replay, 'b>(
        &'b mut self,
        i: &mut &'replay [u8],
    ) -> PResult<PacketType<'replay, 'argtype>> {
        let len = le_u32.parse_next(i)?;
        // A mismatch here means the packet was not really battle-results in this
        // build's layout (packet-id mapping differs by version). Fail gracefully
        // rather than asserting so a single mis-mapped packet can't crash a batch.
        if len as usize != i.len() {
            return Err(failure(ParseError::InvalidPacketData));
        }
        let battle_results: &'replay [u8] = take(len as usize).parse_next(i)?;

        let results = std::str::from_utf8(battle_results).map_err(|e| failure(ParseError::from(e)))?;

        Ok(PacketType::BattleResults(results))
    }

    fn parse_nested_property_update<'replay, 'b>(
        &'b mut self,
        i: &mut &'replay [u8],
    ) -> PResult<PacketType<'replay, 'argtype>> {
        let entity_id = le_u32.parse_next(i)?;
        let is_slice = le_u8.parse_next(i)?;
        let payload_size = le_u32.parse_next(i)?;
        let payload: &[u8] = i;
        if payload_size as usize != payload.len() {
            return Err(failure(ParseError::InvalidPacketData));
        }

        let entity =
            self.entities.get_mut(&entity_id).ok_or_else(|| failure(ParseError::UnknownEntity { entity_id }))?;
        let entity_type = entity.entity_type;

        let spec = &self.specs[entity_type as usize - 1];

        if is_slice & 0xFE != 0 {
            return Err(failure(ParseError::InvalidPacketData));
        }

        let mut reader = crate::nested_property_path::BitReader::new(payload);
        let cont = reader.read_u8(1);
        if cont != 1 {
            return Err(failure(ParseError::InvalidPacketData));
        }
        let prop_idx = reader.read_u8(spec.properties.len().next_power_of_two().trailing_zeros() as u8);
        // `prop_idx` is read with `next_power_of_two` bits, so it can name an index
        // past the spec's real property count on a corrupt or mis-specced packet;
        // bail gracefully instead of indexing `spec.properties` out of bounds.
        if prop_idx as usize >= spec.properties.len() {
            return Err(failure(ParseError::InvalidPacketData));
        }
        if prop_idx as usize >= entity.properties.len() {
            // Properties are not always materialized at entity-create: some stay
            // uninitialized and are created only by a later nested update (e.g.
            // the avatar's `privateVehicleState`, which carries the player's
            // ribbons/buffs and only appears once the first one is earned). Grow
            // the property store with type-default values up to this index so the
            // update below applies, instead of dropping the packet.
            for missing in entity.properties.len()..=prop_idx as usize {
                let default = crate::nested_property_path::default_arg_value(&spec.properties[missing].prop_type);
                entity.properties.push(default);
            }
        }

        let update_cmd = crate::nested_property_path::get_nested_prop_path_helper(
            is_slice & 0x1 == 1,
            &spec.properties[prop_idx as usize].prop_type,
            &mut entity.properties[prop_idx as usize],
            reader,
        )?;

        Ok(PacketType::PropertyUpdate(PropertyUpdatePacket {
            entity_id: entity_id.into(),
            update_cmd,
            property: &spec.properties[prop_idx as usize].name,
        }))
    }

    fn parse_version_packet<'replay, 'b>(&'b self, i: &mut &'replay [u8]) -> PResult<PacketType<'replay, 'argtype>> {
        let len = le_u32.parse_next(i)?;
        let data: &[u8] = take(len as usize).parse_next(i)?;
        let version = std::str::from_utf8(data).map_err(|e| failure(ParseError::from(e)))?;
        Ok(PacketType::Version(version.to_string()))
    }

    fn parse_camera_mode_packet<'replay, 'b>(
        &'b self,
        i: &mut &'replay [u8],
    ) -> PResult<PacketType<'replay, 'argtype>> {
        let mode = le_u32.parse_next(i)?;
        Ok(PacketType::CameraMode(mode))
    }

    fn parse_camera_freelook_packet<'replay, 'b>(
        &'b self,
        i: &mut &'replay [u8],
    ) -> PResult<PacketType<'replay, 'argtype>> {
        let freelook = le_u8.parse_next(i)?;
        Ok(PacketType::CameraFreeLook(freelook))
    }

    fn parse_position_packet<'a, 'b>(&'b self, i: &mut &'a [u8]) -> PResult<PacketType<'a, 'b>> {
        let pid = le_u32.parse_next(i)?;
        let space_id = le_u32.parse_next(i)?;
        let position = parse_vec3.parse_next(i)?;
        let direction = parse_vec3.parse_next(i)?;
        let rotation = parse_rot3.parse_next(i)?;
        let is_on_ground_byte = le_u8.parse_next(i)?;
        let is_on_ground = is_on_ground_byte != 0;
        Ok(PacketType::Position(PositionPacket {
            pid: pid.into(),
            space_id,
            position,
            direction,
            rotation,
            is_on_ground,
        }))
    }

    fn parse_player_orientation_packet<'a, 'b>(&'b self, i: &mut &'a [u8]) -> PResult<PacketType<'a, 'b>> {
        if i.len() != 0x20 {
            return Err(failure(ParseError::InvalidPacketData));
        }
        let pid = le_u32.parse_next(i)?;
        let parent_id = le_u32.parse_next(i)?;
        let position = parse_vec3.parse_next(i)?;
        let rotation = parse_rot3.parse_next(i)?;
        Ok(PacketType::PlayerOrientation(PlayerOrientationPacket {
            pid: pid.into(),
            parent_id: parent_id.into(),
            position,
            rotation,
        }))
    }

    fn parse_camera_packet<'a, 'b>(&'b self, i: &mut &'a [u8]) -> PResult<PacketType<'a, 'b>> {
        // Pre-~0.11 the packet is 56 bytes and omits the `unknown` f32 that sits
        // between `fov` and `position`; later builds added it (60 bytes total).
        // Detect by payload length so both layouts decode.
        let has_unknown = i.len() >= 60;
        let q0 = le_f32.parse_next(i)?;
        let q1 = le_f32.parse_next(i)?;
        let q2 = le_f32.parse_next(i)?;
        let q3 = le_f32.parse_next(i)?;
        let camera_position = parse_vec3.parse_next(i)?;
        let fov = le_f32.parse_next(i)?;
        let unknown = if has_unknown { le_f32.parse_next(i)? } else { -1.0 };
        let position = parse_vec3.parse_next(i)?;
        let direction = parse_vec3.parse_next(i)?;
        Ok(PacketType::Camera(CameraPacket {
            rotation_quat: [q0, q1, q2, q3],
            camera_position,
            fov,
            unknown,
            position,
            direction,
        }))
    }

    fn parse_entity_control_packet<'a, 'b>(&'b self, i: &mut &'a [u8]) -> PResult<PacketType<'a, 'b>> {
        let entity_id = le_u32.parse_next(i)?;
        let is_controlled = le_u8.parse_next(i)?;
        Ok(PacketType::EntityControl(EntityControlPacket {
            entity_id: entity_id.into(),
            is_controlled: is_controlled != 0,
        }))
    }

    fn parse_non_volatile_position_packet<'a, 'b>(&'b self, i: &mut &'a [u8]) -> PResult<PacketType<'a, 'b>> {
        let entity_id = le_u32.parse_next(i)?;
        let space_id = le_u32.parse_next(i)?;
        let position = parse_vec3.parse_next(i)?;
        let rotation = parse_rot3.parse_next(i)?;
        Ok(PacketType::NonVolatilePosition(NonVolatilePositionPacket {
            entity_id: entity_id.into(),
            space_id,
            position,
            rotation,
        }))
    }

    fn parse_player_net_stats_packet<'a, 'b>(&'b self, i: &mut &'a [u8]) -> PResult<PacketType<'a, 'b>> {
        let packed = le_u32.parse_next(i)?;
        let stats = RawPlayerNetStats::from_bytes(packed.to_le_bytes());
        Ok(PacketType::PlayerNetStats(PlayerNetStatsPacket {
            fps: stats.fps(),
            ping: stats.ping(),
            is_lagging: stats.is_lagging(),
        }))
    }

    fn parse_server_timestamp_packet<'a, 'b>(&'b self, i: &mut &'a [u8]) -> PResult<PacketType<'a, 'b>> {
        use winnow::binary::le_f64;
        let timestamp = le_f64.parse_next(i)?;
        Ok(PacketType::ServerTimestamp(ServerTimestampPacket { timestamp }))
    }

    fn parse_server_tick_packet<'a, 'b>(&'b self, i: &mut &'a [u8]) -> PResult<PacketType<'a, 'b>> {
        use winnow::binary::le_f64;
        let tick_rate = le_f64.parse_next(i)?;
        Ok(PacketType::ServerTick(tick_rate))
    }

    fn parse_own_ship_packet<'a, 'b>(&'b self, i: &mut &'a [u8]) -> PResult<PacketType<'a, 'b>> {
        let entity_id = le_u32.parse_next(i)?;
        Ok(PacketType::OwnShip(OwnShipPacket { entity_id: entity_id.into() }))
    }

    fn parse_set_weapon_lock_packet<'a, 'b>(&'b self, i: &mut &'a [u8]) -> PResult<PacketType<'a, 'b>> {
        let weapon_type = le_u32.parse_next(i)?;
        let lock_type = le_u32.parse_next(i)?;
        let target_id = le_u32.parse_next(i)?;
        Ok(PacketType::SetWeaponLock(SetWeaponLockPacket {
            weapon_type: WeaponType::from_raw(weapon_type),
            lock_type: WeaponLockType::from_raw(lock_type),
            target_id: EntityId::from(target_id),
        }))
    }

    fn parse_sub_controller_packet<'a, 'b>(&'b self, i: &mut &'a [u8]) -> PResult<PacketType<'a, 'b>> {
        let mode = le_i16.parse_next(i)?;
        Ok(PacketType::SubController(SubControllerPacket { mode }))
    }

    fn parse_shot_tracking_packet<'a, 'b>(&'b self, i: &mut &'a [u8]) -> PResult<PacketType<'a, 'b>> {
        let entity_id = le_u32.parse_next(i)?;
        let value = le_i64.parse_next(i)?;
        Ok(PacketType::ShotTracking(ShotTrackingPacket { entity_id: entity_id.into(), value }))
    }

    fn parse_gun_marker_packet<'a, 'b>(&'b self, i: &mut &'a [u8]) -> PResult<PacketType<'a, 'b>> {
        let target_point = parse_vec3.parse_next(i)?;
        let diameter = le_f32.parse_next(i)?;
        let marker_position = parse_vec3.parse_next(i)?;
        let marker_direction = parse_vec3.parse_next(i)?;
        let arcade_marker_size = le_f32.parse_next(i)?;
        let spg_param1 = le_f32.parse_next(i)?;
        let spg_param2 = le_f32.parse_next(i)?;
        Ok(PacketType::GunMarker(GunMarkerPacket {
            target_point,
            diameter,
            marker_position,
            marker_direction,
            arcade_marker_size,
            spg_marker_params: (spg_param1, spg_param2),
        }))
    }

    fn parse_unknown_packet<'a, 'b>(&'b self, i: &mut &'a [u8], payload_size: u32) -> PResult<PacketType<'a, 'b>> {
        let contents: &'a [u8] = take(payload_size as usize).parse_next(i)?;
        Ok(PacketType::Unknown(contents))
    }

    fn parse_base_player_create<'a, 'b>(&'b mut self, i: &mut &'a [u8]) -> PResult<PacketType<'a, 'b>> {
        let entity_id = le_u32.parse_next(i)?;
        let entity_type = le_u16.parse_next(i)?;
        let spec = (entity_type as usize)
            .checked_sub(1)
            .and_then(|idx| self.specs.get(idx))
            .ok_or_else(|| failure(ParseError::EntityTypeOutOfBounds { entity_type, spec_count: self.specs.len() }))?;

        let mut props: HashMap<&str, _> = HashMap::new();
        let mut stored_props: Vec<_> = vec![];
        for prop_id in 0..spec.base_properties.len() {
            let spec = &spec.base_properties[prop_id];
            let value = match spec.prop_type.parse_value(i) {
                Ok(x) => x,
                Err(e) => {
                    return Err(failure(ParseError::RpcValueParseFailed {
                        method: format!("BasePlayerCreate::{}", spec.name),
                        argnum: prop_id,
                        argtype: format!("{:?}", spec),
                        semantic_name: spec.prop_type.semantic_name().map(str::to_string),
                        error: format!("{:?}", e),
                    }));
                }
            };
            stored_props.push(value.clone());
            props.insert(&spec.name, value);
        }

        let component_data = i.to_vec();
        // Consume remaining input
        *i = &[];

        self.entities.insert(
            entity_id,
            Entity {
                entity_type,
                // TODO: Parse the state
                properties: stored_props,
            },
        );
        Ok(PacketType::BasePlayerCreate(BasePlayerCreatePacket {
            entity_id: entity_id.into(),
            entity_type: &spec.name,
            props,
            component_data,
        }))
    }

    /// Parse a bare BasePlayerCreate (packet 0x26) that carries no inline base properties.
    /// Unlike packet 0x0, this packet only has entity_id + entity_type + a small data blob
    /// (typically 4 zero bytes). The entity receives its real property values later via
    /// property-update packets or gets superseded by a full BasePlayerCreate (packet 0x0).
    fn parse_base_player_create_stub<'a, 'b>(&'b mut self, i: &mut &'a [u8]) -> PResult<PacketType<'a, 'b>> {
        let entity_id = le_u32.parse_next(i)?;
        let entity_type = le_u16.parse_next(i)?;
        let spec = (entity_type as usize)
            .checked_sub(1)
            .and_then(|idx| self.specs.get(idx))
            .ok_or_else(|| failure(ParseError::EntityTypeOutOfBounds { entity_type, spec_count: self.specs.len() }))?;

        let component_data = i.to_vec();
        *i = &[];

        self.entities.insert(entity_id, Entity { entity_type, properties: vec![] });
        Ok(PacketType::BasePlayerCreate(BasePlayerCreatePacket {
            entity_id: entity_id.into(),
            entity_type: &spec.name,
            props: HashMap::new(),
            component_data,
        }))
    }

    fn parse_entity_create<'a, 'b>(&'b mut self, i: &mut &'a [u8]) -> PResult<PacketType<'a, 'b>> {
        let entity_id = le_u32.parse_next(i)?;
        let entity_type = le_u16.parse_next(i)?;
        let vehicle_id = le_u32.parse_next(i)?;
        let space_id = le_u32.parse_next(i)?;
        let position = parse_vec3.parse_next(i)?;
        let rotation = parse_rot3.parse_next(i)?;
        let state_length = le_u32.parse_next(i)?;
        let state: &'a [u8] = take(i.len()).parse_next(i)?;
        if self.entities.contains_key(&entity_id) {
            //println!("DBG: Entity {} got created twice!", entity_id);
        }

        let mut sub = state;
        let num_props = le_u8.parse_next(&mut sub)?;
        let mut props: HashMap<&str, _> = HashMap::new();
        let mut stored_props: Vec<_> = vec![];
        // Resolve the entity's spec up front; sibling handlers return structured
        // errors here rather than panicking on a bad index. parse_packet wraps
        // any Err into an Invalid packet so the stream keeps parsing. With specs
        // matching the replay version these bounds always hold, so a failure here
        // signals mismatched specs or a decoder bug, not normal version drift.
        let entity_spec = (entity_type as usize)
            .checked_sub(1)
            .and_then(|idx| self.specs.get(idx))
            .ok_or_else(|| failure(ParseError::EntityTypeOutOfBounds { entity_type, spec_count: self.specs.len() }))?;
        for _ in 0..num_props {
            let prop_id = le_u8.parse_next(&mut sub)?;
            let spec = entity_spec
                .properties
                .get(prop_id as usize)
                .ok_or_else(|| failure(ParseError::PropertyIdOutOfBounds { prop_id: prop_id as u32, entity_type }))?;
            let value = match spec.prop_type.parse_value(&mut sub) {
                Ok(x) => x,
                Err(e) => {
                    return Err(failure(ParseError::RpcValueParseFailed {
                        method: format!("EntityCreate::{}", spec.name),
                        argnum: prop_id as usize,
                        argtype: format!("{:?}", spec),
                        semantic_name: spec.prop_type.semantic_name().map(str::to_string),
                        error: format!("{:?}", e),
                    }));
                }
            };
            stored_props.push(value.clone());
            props.insert(&spec.name, value);
        }

        self.entities.insert(entity_id, Entity { entity_type, properties: stored_props });

        Ok(PacketType::EntityCreate(EntityCreatePacket {
            entity_id: entity_id.into(),
            spec_idx: entity_type as usize,
            entity_type: &entity_spec.name,
            space_id,
            vehicle_id: vehicle_id.into(),
            position,
            rotation,
            state_length,
            props,
        }))
    }

    fn parse_cell_player_create<'a, 'b>(&'b mut self, i: &mut &'a [u8]) -> PResult<PacketType<'a, 'b>> {
        let entity_id = le_u32.parse_next(i)?;
        let space_id = le_u32.parse_next(i)?;
        // let _unknown = le_u16.parse_next(i)?;
        let vehicle_id = le_u32.parse_next(i)?;
        let position = parse_vec3.parse_next(i)?;
        let rotation = parse_rot3.parse_next(i)?;
        let props_len = le_u32.parse_next(i)?;
        let props_data: &'a [u8] = take(props_len as usize).parse_next(i)?;

        if !self.entities.contains_key(&entity_id) {
            return Err(failure(ParseError::UnknownEntity { entity_id }));
        }

        // The value can be parsed into all internal properties
        /*println!(
            "{} {} {} {} {},{},{} {},{},{} value.len()={}",
            entity_id,
            space_id,
            5, //unknown,
            vehicle_id,
            posx,
            posy,
            posz,
            dirx,
            diry,
            dirz,
            value.len()
        );*/
        let entity_type = self.entities.get(&entity_id).unwrap().entity_type;
        let spec = &self.specs[entity_type as usize - 1];

        let mut sub = props_data;
        let mut props: HashMap<&str, _> = HashMap::new();
        let mut stored_props: Vec<_> = vec![];
        for prop_id in 0..spec.internal_properties.len() {
            let spec = &spec.internal_properties[prop_id];
            let value = match spec.prop_type.parse_value(&mut sub) {
                Ok(x) => x,
                Err(e) => {
                    return Err(failure(ParseError::RpcValueParseFailed {
                        method: format!("CellPlayerCreate::{}", spec.name),
                        argnum: prop_id,
                        argtype: format!("{:?}", spec),
                        semantic_name: spec.prop_type.semantic_name().map(str::to_string),
                        error: format!("{:?}", e),
                    }));
                }
            };
            stored_props.push(value.clone());
            props.insert(&spec.name, value);
        }

        let component_data = sub.to_vec();

        Ok(PacketType::CellPlayerCreate(CellPlayerCreatePacket {
            entity_id: entity_id.into(),
            entity_type: &spec.name,
            vehicle_id: vehicle_id.into(),
            space_id,
            position,
            rotation,
            props,
            component_data,
        }))
    }

    fn parse_entity_leave<'a, 'b>(&'b self, i: &mut &'a [u8]) -> PResult<PacketType<'a, 'b>> {
        let entity_id = le_u32.parse_next(i)?;
        Ok(PacketType::EntityLeave(EntityLeavePacket { entity_id: entity_id.into() }))
    }

    fn parse_entity_enter<'a, 'b>(&'b mut self, i: &mut &'a [u8]) -> PResult<PacketType<'a, 'b>> {
        let entity_id = le_u32.parse_next(i)?;
        let space_id = le_u32.parse_next(i)?;
        let vehicle_id = le_u32.parse_next(i)?;
        Ok(PacketType::EntityEnter(EntityEnterPacket {
            entity_id: entity_id.into(),
            space_id,
            vehicle_id: vehicle_id.into(),
        }))
    }

    fn parse_cruise_state<'a, 'b>(&'b mut self, i: &mut &'a [u8]) -> PResult<PacketType<'a, 'b>> {
        let key = le_u32.parse_next(i)?;
        let value = le_i32.parse_next(i)?;
        Ok(PacketType::CruiseState(CruiseState { key, value }))
    }

    fn parse_map_packet<'a, 'b>(&'b mut self, i: &mut &'a [u8]) -> PResult<PacketType<'a, 'b>> {
        let space_id = le_u32.parse_next(i)?;
        let arena_id = le_i64.parse_next(i)?;
        let unknown1 = le_u32.parse_next(i)?;
        let unknown2 = le_u32.parse_next(i)?;
        let blob: &'a [u8] = take(128usize).parse_next(i)?;
        let string_size = le_u32.parse_next(i)?;
        let map_name: &'a [u8] = take(string_size as usize).parse_next(i)?;
        let map_name = std::str::from_utf8(map_name).map_err(|e| failure(ParseError::from(e)))?;
        let matrix: &'a [u8] = take(4usize * 4 * 4).parse_next(i)?;
        let unknown = le_u8.parse_next(i)?;
        let packet = MapPacket { space_id, arena_id, unknown1, unknown2, blob, map_name, matrix, unknown };
        Ok(PacketType::Map(packet))
    }

    fn parse_naked_packet<'a, 'b>(
        &'b mut self,
        packet_type: PacketTypeId,
        i: &mut &'a [u8],
    ) -> PResult<PacketType<'a, 'b>> {
        /*
        PACKETS_MAPPING = {
            0x0: BasePlayerCreate,
            0x1: CellPlayerCreate,
            0x2: EntityControl,
            0x3: EntityEnter,
            0x4: EntityLeave,
            0x5: EntityCreate,
            # 0x6
            0x7: EntityProperty,
            0x8: EntityMethod,
            0x27: Map,
            0x22: NestedProperty,
            0x0a: Position
        }
        */
        let payload = match packet_type {
            PacketTypeId::BasePlayerCreate => self.parse_base_player_create(i)?,
            PacketTypeId::BasePlayerCreateStub => self.parse_base_player_create_stub(i)?,
            PacketTypeId::CellPlayerCreate => self.parse_cell_player_create(i)?,
            PacketTypeId::EntityControl => self.parse_entity_control_packet(i)?,
            PacketTypeId::EntityEnter => self.parse_entity_enter(i)?,
            PacketTypeId::EntityLeave => self.parse_entity_leave(i)?,
            PacketTypeId::EntityCreate => self.parse_entity_create(i)?,
            PacketTypeId::EntityProperty => self.parse_entity_property_packet(i)?,
            PacketTypeId::EntityMethod => self.parse_entity_method_packet(i)?,
            PacketTypeId::Position => self.parse_position_packet(i)?,
            PacketTypeId::ServerTick => self.parse_server_tick_packet(i)?,
            PacketTypeId::ServerTimestamp => self.parse_server_timestamp_packet(i)?,
            PacketTypeId::Version => self.parse_version_packet(i)?,
            PacketTypeId::PlayerNetStats => self.parse_player_net_stats_packet(i)?,
            PacketTypeId::OwnShip => self.parse_own_ship_packet(i)?,
            PacketTypeId::BattleResults => self.parse_battle_results(i)?,
            PacketTypeId::NestedPropertyUpdate => self.parse_nested_property_update(i)?,
            PacketTypeId::Camera => self.parse_camera_packet(i)?,
            PacketTypeId::CameraMode => self.parse_camera_mode_packet(i)?,
            PacketTypeId::Map => self.parse_map_packet(i)?,
            PacketTypeId::NonVolatilePosition => self.parse_non_volatile_position_packet(i)?,
            PacketTypeId::PlayerOrientation => self.parse_player_orientation_packet(i)?,
            PacketTypeId::CameraFreeLook => self.parse_camera_freelook_packet(i)?,
            PacketTypeId::SetWeaponLock => self.parse_set_weapon_lock_packet(i)?,
            PacketTypeId::SubController => self.parse_sub_controller_packet(i)?,
            PacketTypeId::CruiseState => self.parse_cruise_state(i)?,
            PacketTypeId::ShotTracking => self.parse_shot_tracking_packet(i)?,
            PacketTypeId::GunMarker => self.parse_gun_marker_packet(i)?,
            PacketTypeId::InitFlag => {
                let flag = le_u8.parse_next(i)?;
                PacketType::InitFlag(flag)
            }
            PacketTypeId::InitMarker => {
                // Consume all remaining input (empty init marker)
                *i = &[];
                PacketType::InitMarker
            }
            PacketTypeId::Unknown(_) => self.parse_unknown_packet(i, (*i).len().try_into().unwrap())?,
        };
        Ok(payload)
    }

    pub fn parse_packet<'a, 'b>(&'b mut self, i: &mut &'a [u8]) -> PResult<Packet<'a, 'b>> {
        let packet_size = le_u32.parse_next(i)?;
        let raw_type = le_u32.parse_next(i)?;
        let packet_type = match &self.version {
            Some(version) => PacketTypeId::from_raw_for_version(raw_type, version),
            None => PacketTypeId::from_raw(raw_type),
        };
        let raw_clock = le_f32.parse_next(i)?;
        let clock = GameClock(raw_clock);
        let packet_data: &'a [u8] = take(packet_size as usize).parse_next(i)?;
        let raw = packet_data;
        let mut sub = packet_data;
        let (leftover, payload) = match self.parse_naked_packet(packet_type, &mut sub) {
            Ok(payload) => (sub, payload),
            Err(winnow::error::ErrMode::Cut(ParseError::UnsupportedReplayVersion { version })) => {
                return Err(failure(ParseError::UnsupportedReplayVersion { version }));
            }
            // Method-id drift is never a single bad packet; it means the whole
            // exposed-method table is misaligned, so surface it loudly instead of
            // burying it as one Invalid packet among the resulting garbage.
            Err(winnow::error::ErrMode::Cut(drift @ ParseError::ExposedMethodMappingDrift { .. })) => {
                return Err(failure(drift));
            }
            Err(e) => {
                (
                    &packet_data[0..0], // Empty reference
                    PacketType::Invalid(InvalidPacket { message: format!("{:?}", e), raw: packet_data }),
                )
            }
        };
        Ok(Packet { packet_size, packet_type, clock, payload, raw, leftover })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn version(major: u32, minor: u32, patch: u32) -> Version {
        Version::base(major, minor, patch)
    }

    /// The layout boundary is at 12.6.0: 12.5.0 and earlier use the legacy
    /// packet-ID table, 12.6.0 and later the modern one. Confirmed empirically
    /// from per-version raw packet-ID histograms (32-byte PlayerOrientation and
    /// 60-byte Camera packets pin the layout): 12.3.1/12.4.0/12.5.0 carry
    /// PlayerOrientation at 0x2b and Camera at 0x24; 12.6.0 shifts them to 0x2c
    /// and 0x25. Builds 12.4.0/12.5.0 once mis-mapped to the modern table and
    /// produced zero decodable position tracks.
    #[test]
    fn layout_boundary_is_12_6_0() {
        // Legacy side: PlayerOrientation 0x2b, Camera 0x24, SetWeaponLock 0x2f.
        for v in [version(12, 3, 1), version(12, 4, 0), version(12, 5, 0)] {
            assert_eq!(PacketTypeId::from_raw_for_version(0x2b, &v), PacketTypeId::PlayerOrientation);
            assert_eq!(PacketTypeId::from_raw_for_version(0x24, &v), PacketTypeId::Camera);
            assert_eq!(PacketTypeId::from_raw_for_version(0x2f, &v), PacketTypeId::SetWeaponLock);
            assert_eq!(PacketTypeId::from_raw_for_version(0x22, &v), PacketTypeId::NestedPropertyUpdate);
        }

        // Modern side: everything from 0x23 up shifts one slot, BattleResults
        // claims 0x22.
        for v in [version(12, 6, 0), version(13, 0, 0), version(15, 4, 0)] {
            assert_eq!(PacketTypeId::from_raw_for_version(0x2c, &v), PacketTypeId::PlayerOrientation);
            assert_eq!(PacketTypeId::from_raw_for_version(0x25, &v), PacketTypeId::Camera);
            assert_eq!(PacketTypeId::from_raw_for_version(0x30, &v), PacketTypeId::SetWeaponLock);
            assert_eq!(PacketTypeId::from_raw_for_version(0x22, &v), PacketTypeId::BattleResults);
            assert_eq!(PacketTypeId::from_raw_for_version(0x23, &v), PacketTypeId::NestedPropertyUpdate);
        }
    }

    /// Packets below the shift (0x00..=0x21) are identical in both layouts, so
    /// version gating must not perturb them.
    #[test]
    fn pre_shift_packets_are_layout_invariant() {
        let legacy = version(12, 5, 0);
        let modern = version(12, 6, 0);
        for raw in [0x00, 0x01, 0x05, 0x07, 0x08, 0x0a, 0x18, 0x1d, 0x20] {
            assert_eq!(
                PacketTypeId::from_raw_for_version(raw, &legacy),
                PacketTypeId::from_raw_for_version(raw, &modern),
                "raw 0x{raw:02x} should map identically across the layout shift"
            );
        }
    }

    /// Every mapped variant round-trips through `raw()` in the modern table.
    #[test]
    fn modern_raw_roundtrip() {
        for raw in 0x00..=0x40u32 {
            let id = PacketTypeId::from_raw_modern(raw);
            if !matches!(id, PacketTypeId::Unknown(_)) {
                assert_eq!(id.raw(), raw, "0x{raw:02x} did not round-trip");
            }
        }
    }

    /// Regression: an EntityCreate whose `entity_type` is out of range for the
    /// loaded specs must return a structured error, not panic. This used to
    /// index `self.specs[entity_type - 1]` directly and crashed the process
    /// when the parser was built with empty specs (the extracted-dir loader
    /// path). `parse_packet` relies on the error to degrade the packet to
    /// `Invalid` and keep parsing.
    #[test]
    fn entity_create_out_of_range_entity_type_errors_instead_of_panicking() {
        let specs: Vec<EntitySpec> = Vec::new();
        let mut parser = Parser::new(&specs);

        let mut payload: Vec<u8> = Vec::new();
        payload.extend_from_slice(&0u32.to_le_bytes()); // entity_id
        payload.extend_from_slice(&2u16.to_le_bytes()); // entity_type (out of range: specs is empty)
        payload.extend_from_slice(&0u32.to_le_bytes()); // vehicle_id
        payload.extend_from_slice(&0u32.to_le_bytes()); // space_id
        payload.extend_from_slice(&[0u8; 12]); // position
        payload.extend_from_slice(&[0u8; 12]); // rotation
        payload.extend_from_slice(&0u32.to_le_bytes()); // state_length
        payload.push(1); // state blob: num_props = 1

        let mut slice = payload.as_slice();
        let result = parser.parse_entity_create(&mut slice);
        assert!(
            matches!(
                result,
                Err(winnow::error::ErrMode::Cut(ParseError::EntityTypeOutOfBounds { entity_type: 2, spec_count: 0 }))
            ),
            "expected EntityTypeOutOfBounds, got {result:?}"
        );
    }
}

#[cfg(test)]
mod diag_test {
    use super::PayloadDiagnostic;

    #[test]
    fn leftover_produces_diagnostic() {
        // Consumed fewer bytes than the payload held => a diagnostic.
        let d = PayloadDiagnostic::for_leftover(
            "EntityMethod::Avatar::onFoo".to_string(),
            Some("ENTITY_ID".to_string()),
            8,
            4,
        );
        let d = d.expect("expected a diagnostic");
        assert_eq!(d.payload_len, 8);
        assert_eq!(d.consumed, 4);
        assert_eq!(d.semantic_name.as_deref(), Some("ENTITY_ID"));

        // Fully consumed => no diagnostic.
        assert!(PayloadDiagnostic::for_leftover("ctx".to_string(), None, 8, 8).is_none());
    }
}

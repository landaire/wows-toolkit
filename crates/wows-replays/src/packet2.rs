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
use std::collections::HashMap;
use std::convert::TryInto;

use crate::error::*;
use crate::types::EntityId;
use crate::types::GameClock;
use crate::types::GameParamId;
use wowsunpack::rpc::entitydefs::*;
use wowsunpack::rpc::typedefs::ArgValue;

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

/// Packet 0x30: `onSetWeaponLock` — weapon lock state change. Triggers
/// `setWeaponLockCallback` in the Python layer. Confirmed via
/// `WGReplayController::_py_onSetWeaponLock`.
///
/// Fields derived from `weaponLockFlags`, `WeaponType` enum, and `LOCK_NONE`
/// constants in the game client.
#[derive(Debug, Serialize, Clone)]
pub struct SetWeaponLockPacket {
    /// Lock state (0 = LOCK_NONE / unlock).
    pub lock_state: u32,
    /// Weapon type from `WeaponType` enum (e.g. ARTILLERY, TORPEDO, WAVES).
    pub weapon_type: u32,
    /// Entity ID of the lock target.
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

/// First build using the modern BigWorld packet-ID layout where `BattleResults`
/// occupies `0x22` and every packet at `0x23` and beyond shifted up by one slot
/// from the legacy layout.
///
/// Determined empirically from per-version packet-ID histograms: build 6965290
/// (12.3.1) is the latest known to use the legacy layout; build 7499736 (12.8.0)
/// is the earliest known to use the modern layout. Picking 12.4.0 as the cutoff
/// is the cleanest convention; revise if a replay between 12.3.1 and 12.4.0
/// turns out to use the modern layout.
pub const MODERN_PACKET_MAPPING_MIN_BUILD: u32 = 7062104;

impl PacketTypeId {
    /// Map a raw wire identifier to a `PacketTypeId` using the modern layout
    /// (build >= [`MODERN_PACKET_MAPPING_MIN_BUILD`]). Unrecognised values
    /// become `Unknown(raw)`.
    pub fn from_raw(raw: u32) -> Self {
        Self::from_raw_for_build(raw, u32::MAX)
    }

    /// Build-aware variant of [`Self::from_raw`]. Before
    /// [`MODERN_PACKET_MAPPING_MIN_BUILD`] the wire layout had no `BattleResults`
    /// packet and ran each subsequent ID one slot lower (0x22 was
    /// NestedPropertyUpdate, 0x24 Camera, 0x27 Map, 0x2c PlayerOrientation, etc.).
    pub fn from_raw_for_build(raw: u32, build: u32) -> Self {
        if build >= MODERN_PACKET_MAPPING_MIN_BUILD { Self::from_raw_modern(raw) } else { Self::from_raw_legacy(raw) }
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
    build: u32,
}

impl<'a> RawPacketIterator<'a> {
    /// Walk a packet stream assuming the modern packet-ID layout. For replays
    /// from older builds, use [`Self::with_build`] so packet IDs dispatch
    /// correctly across the protocol shift documented at
    /// [`MODERN_PACKET_MAPPING_MIN_BUILD`].
    pub fn new(packet_data: &'a [u8]) -> Self {
        Self { remaining: packet_data, build: u32::MAX }
    }

    /// Walk a packet stream using the packet-ID layout that matches `build`.
    pub fn with_build(packet_data: &'a [u8], build: u32) -> Self {
        Self { remaining: packet_data, build }
    }
}

impl<'a> Iterator for RawPacketIterator<'a> {
    type Item = PResult<RawPacket<'a>>;

    fn next(&mut self) -> Option<Self::Item> {
        if self.remaining.is_empty() {
            return None;
        }
        let build = self.build;
        let result = (|| {
            let packet_size = le_u32.parse_next(&mut self.remaining)?;
            let packet_type = PacketTypeId::from_raw_for_build(le_u32.parse_next(&mut self.remaining)?, build);
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
    build: u32,
}

impl<'argtype> Parser<'argtype> {
    /// Build a parser assuming the modern packet-ID layout. Equivalent to
    /// [`Self::with_build`] with `u32::MAX`; prefer that variant when the
    /// replay's build is known so old replays dispatch correctly across the
    /// protocol shift at [`MODERN_PACKET_MAPPING_MIN_BUILD`].
    pub fn new(entities: &'argtype [EntitySpec]) -> Parser<'argtype> {
        Self::with_build(entities, u32::MAX)
    }

    /// Build a parser that dispatches packet IDs for the given build number.
    pub fn with_build(entities: &'argtype [EntitySpec], build: u32) -> Parser<'argtype> {
        Parser { specs: entities, entities: HashMap::new(), build }
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

        let pval = spec.prop_type.parse_value(&mut &*payload).map_err(|e| {
            failure(ParseError::RpcValueParseFailed {
                method: format!("EntityProperty::{}", spec.name),
                argnum: prop_id as usize,
                argtype: format!("{:?}", spec.prop_type),
                error: format!("{e:?}"),
            })
        })?;

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
        assert!(i.is_empty());

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
        assert_eq!(len as usize, i.len());
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
        assert_eq!(payload_size as usize, payload.len());

        let entity = self.entities.get_mut(&entity_id).unwrap();
        let entity_type = entity.entity_type;

        let spec = &self.specs[entity_type as usize - 1];

        assert!(is_slice & 0xFE == 0);

        let mut reader = crate::nested_property_path::BitReader::new(payload);
        let cont = reader.read_u8(1);
        assert!(cont == 1);
        let prop_idx = reader.read_u8(spec.properties.len().next_power_of_two().trailing_zeros() as u8);
        if prop_idx as usize >= entity.properties.len() {
            // This is almost certainly a nested property set on the player avatar.
            // Currently, we assume that all properties are created when the entity is
            // created. However, apparently the properties can go un-initialized at the
            // beginning, and then later get created by a nested property update.
            //
            // We should do two things:
            // - Store the entity's properties as a HashMap
            // - Separate finding the path from updating the property value, and then here
            //   we can create the entry if the property hasn't been created yet.
            return Err(failure(ParseError::UnsupportedInternalPropSet { entity_id, entity_type: spec.name.clone() }));
        }

        let update_cmd = crate::nested_property_path::get_nested_prop_path_helper(
            is_slice & 0x1 == 1,
            &spec.properties[prop_idx as usize].prop_type,
            &mut entity.properties[prop_idx as usize],
            reader,
        );

        Ok(PacketType::PropertyUpdate(PropertyUpdatePacket {
            entity_id: entity_id.into(),
            update_cmd,
            property: &spec.properties[prop_idx as usize].name,
        }))
    }

    fn parse_version_packet<'replay, 'b>(&'b self, i: &mut &'replay [u8]) -> PResult<PacketType<'replay, 'argtype>> {
        let len = le_u32.parse_next(i)?;
        let data: &[u8] = take(len as usize).parse_next(i)?;
        Ok(PacketType::Version(std::str::from_utf8(data).unwrap().to_string()))
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
        assert!(i.len() == 0x20);
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
        let q0 = le_f32.parse_next(i)?;
        let q1 = le_f32.parse_next(i)?;
        let q2 = le_f32.parse_next(i)?;
        let q3 = le_f32.parse_next(i)?;
        let camera_position = parse_vec3.parse_next(i)?;
        let fov = le_f32.parse_next(i)?;
        let unknown = le_f32.parse_next(i)?;
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
        let lock_state = le_u32.parse_next(i)?;
        let weapon_type = le_u32.parse_next(i)?;
        let target_id = le_u32.parse_next(i)?;
        Ok(PacketType::SetWeaponLock(SetWeaponLockPacket {
            lock_state,
            weapon_type,
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
        let spec = &self.specs[entity_type as usize - 1];

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
        let spec = &self.specs[entity_type as usize - 1];

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
        for _ in 0..num_props {
            let prop_id = le_u8.parse_next(&mut sub)?;
            let spec = &self.specs[entity_type as usize - 1].properties[prop_id as usize];
            let value = match spec.prop_type.parse_value(&mut sub) {
                Ok(x) => x,
                Err(e) => {
                    return Err(failure(ParseError::RpcValueParseFailed {
                        method: format!("EntityCreate::{}", spec.name),
                        argnum: prop_id as usize,
                        argtype: format!("{:?}", spec),
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
            entity_type: &self.specs[entity_type as usize - 1].name,
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
            panic!("Cell player, entity id {}, was created before base player!", entity_id);
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
        let matrix: &'a [u8] = take(4usize * 4 * 4).parse_next(i)?;
        let unknown = le_u8.parse_next(i)?;
        let packet = MapPacket {
            space_id,
            arena_id,
            unknown1,
            unknown2,
            blob,
            // TODO: Use a winnow combinator for this (for error handling)
            map_name: std::str::from_utf8(map_name).unwrap(),
            matrix,
            unknown,
        };
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
        let packet_type = PacketTypeId::from_raw_for_build(le_u32.parse_next(i)?, self.build);
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

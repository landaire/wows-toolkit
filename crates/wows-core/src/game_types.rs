//! Game concept types that describe World of Warships mechanics.
//!
//! These types represent game entities, identifiers, positions, and enumerations
//! that are useful across any tool working with WoWS data -- not just replay parsers.

use std::fmt;

use crate::Version;
use crate::game_constants::BattleConstants;
use crate::game_constants::CommonConstants;
use crate::game_constants::ShipsConstants;
use crate::recognized::Recognized;

use crate::units::Meters;

// =============================================================================
// Identity Types
// =============================================================================

/// Per-replay-session entity identifier for game objects (ships, buildings, smoke screens).
/// The wire format is u32 but some packet types use i32 or i64.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "serde", serde(transparent))]
#[cfg_attr(feature = "rkyv", derive(rkyv::Archive, rkyv::Serialize, rkyv::Deserialize))]
pub struct EntityId(u32);

impl EntityId {
    pub fn raw(self) -> u32 {
        self.0
    }
}

impl fmt::Display for EntityId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl From<u32> for EntityId {
    fn from(v: u32) -> Self {
        EntityId(v)
    }
}

impl From<i32> for EntityId {
    fn from(v: i32) -> Self {
        EntityId(v as u32)
    }
}

impl From<i64> for EntityId {
    fn from(v: i64) -> Self {
        EntityId(v as u32)
    }
}

/// Index of a gun within a ship weapon component's gun list.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "serde", serde(transparent))]
#[cfg_attr(feature = "rkyv", derive(rkyv::Archive, rkyv::Serialize, rkyv::Deserialize))]
pub struct GunId(u32);

impl GunId {
    pub fn raw(self) -> u32 {
        self.0
    }

    /// Index into a per-gun array (e.g. atbaTargets).
    pub fn index(self) -> usize {
        self.0 as usize
    }
}

impl From<u32> for GunId {
    fn from(v: u32) -> Self {
        GunId(v)
    }
}

/// Raw per-gun fire bitmask: bit `g` set means gun `g` fired (`bits & (1 << g)`).
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, Hash)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "serde", serde(transparent))]
#[cfg_attr(feature = "rkyv", derive(rkyv::Archive, rkyv::Serialize, rkyv::Deserialize))]
pub struct GunBits(u32);

impl GunBits {
    pub fn raw(self) -> u32 {
        self.0
    }

    /// Gun indices whose bit is set, ascending.
    pub fn gun_ids(self) -> impl Iterator<Item = GunId> {
        (0..u32::BITS).filter(move |g| self.0 & (1u32 << g) != 0).map(GunId::from)
    }
}

impl From<u32> for GunBits {
    fn from(v: u32) -> Self {
        GunBits(v)
    }
}

/// Entity identifier for the client-side Avatar entity.
///
/// In WoWs replays the recording player has two entities: a Vehicle (the ship,
/// tracked by `EntityId`) and an Avatar (the client object that receives RPC
/// methods like `receiveShotKills`, `receiveArtilleryShots`, etc.).
/// This type distinguishes avatar entity IDs from vehicle/ship entity IDs to
/// prevent silent mismatches.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "serde", serde(transparent))]
#[cfg_attr(feature = "rkyv", derive(rkyv::Archive, rkyv::Serialize, rkyv::Deserialize))]
pub struct AvatarId(u32);

impl AvatarId {
    pub fn raw(self) -> u32 {
        self.0
    }
}

impl fmt::Display for AvatarId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "avatar:{}", self.0)
    }
}

impl From<EntityId> for AvatarId {
    fn from(eid: EntityId) -> Self {
        AvatarId(eid.raw())
    }
}

impl From<u32> for AvatarId {
    fn from(v: u32) -> Self {
        AvatarId(v)
    }
}

impl From<i32> for AvatarId {
    fn from(v: i32) -> Self {
        AvatarId(v as u32)
    }
}

impl From<i64> for AvatarId {
    fn from(v: i64) -> Self {
        AvatarId(v as u32)
    }
}

/// Team identifier within a battle. Always 0 or 1 in two-team modes; a few
/// match types use higher values for neutral / observer teams. The newtype
/// exists so functions like "the recording player's team" don't accidentally
/// get confused with other ints (entity ids, account ids, raw building
/// team_ids that survived as i8 elsewhere).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "serde", serde(transparent))]
#[cfg_attr(feature = "rkyv", derive(rkyv::Archive, rkyv::Serialize, rkyv::Deserialize))]
pub struct TeamId(i64);

impl TeamId {
    pub fn new(v: i64) -> Self {
        TeamId(v)
    }

    pub fn raw(self) -> i64 {
        self.0
    }
}

impl fmt::Display for TeamId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl From<i64> for TeamId {
    fn from(v: i64) -> Self {
        TeamId(v)
    }
}

impl From<i32> for TeamId {
    fn from(v: i32) -> Self {
        TeamId(v as i64)
    }
}

impl From<i8> for TeamId {
    fn from(v: i8) -> Self {
        TeamId(v as i64)
    }
}

impl From<u32> for TeamId {
    fn from(v: u32) -> Self {
        TeamId(v as i64)
    }
}

/// Server-assigned identifier for a single match instance ("arena").
///
/// Every client recording the same match observes the same arena id; comparing
/// across replays is the cheapest reliable way to confirm two replays come from
/// the same battle.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "serde", serde(transparent))]
#[cfg_attr(feature = "rkyv", derive(rkyv::Archive, rkyv::Serialize, rkyv::Deserialize))]
pub struct ArenaId(i64);

impl ArenaId {
    pub fn new(v: i64) -> Self {
        ArenaId(v)
    }

    pub fn raw(self) -> i64 {
        self.0
    }
}

impl fmt::Display for ArenaId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl From<i64> for ArenaId {
    fn from(v: i64) -> Self {
        ArenaId(v)
    }
}

/// A persistent player account identifier (db_id, avatar_id).
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "serde", serde(transparent))]
#[cfg_attr(feature = "rkyv", derive(rkyv::Archive, rkyv::Serialize, rkyv::Deserialize))]
pub struct AccountId(pub i64);

impl AccountId {
    pub fn raw(self) -> i64 {
        self.0
    }
}

impl fmt::Display for AccountId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl From<u32> for AccountId {
    fn from(v: u32) -> Self {
        AccountId(v as i64)
    }
}

impl From<i32> for AccountId {
    fn from(v: i32) -> Self {
        AccountId(v as i64)
    }
}

impl From<i64> for AccountId {
    fn from(v: i64) -> Self {
        AccountId(v)
    }
}

/// A game parameter type identifier from GameParams (ships, equipment, etc.).
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, Hash)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "serde", serde(transparent))]
#[cfg_attr(feature = "rkyv", derive(rkyv::Archive, rkyv::Serialize, rkyv::Deserialize))]
pub struct GameParamId(u64);

impl GameParamId {
    pub fn raw(self) -> u64 {
        self.0
    }
}

impl fmt::Display for GameParamId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl From<u32> for GameParamId {
    fn from(v: u32) -> Self {
        GameParamId(v as u64)
    }
}

impl From<u64> for GameParamId {
    fn from(v: u64) -> Self {
        GameParamId(v)
    }
}

impl From<i64> for GameParamId {
    fn from(v: i64) -> Self {
        GameParamId(v as u64)
    }
}

/// Represents the relation of a player/entity to the recording player.
/// Corresponds to `PLAYER_RELATION` in battle.xml:
/// - 0 = SELF (the player who recorded the replay)
/// - 1 = ALLY (teammate)
/// - 2 = ENEMY
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "serde", serde(transparent))]
#[cfg_attr(feature = "rkyv", derive(rkyv::Archive, rkyv::Serialize, rkyv::Deserialize))]
pub struct Relation(u32);

impl Relation {
    pub fn new(value: u32) -> Self {
        Self(value)
    }

    pub fn is_self(&self) -> bool {
        self.0 == 0
    }

    pub fn is_ally(&self) -> bool {
        self.0 == 1
    }

    pub fn is_enemy(&self) -> bool {
        self.0 >= 2
    }

    pub fn name(&self) -> &'static str {
        match self.0 {
            0 => "Self",
            1 => "Ally",
            2 => "Enemy",
            _ => "Unknown",
        }
    }

    pub fn value(&self) -> u32 {
        self.0
    }
}

impl fmt::Display for Relation {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.name())
    }
}

impl From<u32> for Relation {
    fn from(value: u32) -> Self {
        Self(value)
    }
}

/// Lock mode for a weapon, from the `WeaponLocks` enum in the game client
/// (`scripts/Components/__init__.pyc`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "rkyv", derive(rkyv::Archive, rkyv::Serialize, rkyv::Deserialize))]
pub enum WeaponLockType {
    /// `LOCK_NONE` — no lock (an unlock).
    None,
    /// `LOCK_ABSOLUTE` — lock onto a fixed point in world space.
    Absolute,
    /// `LOCK_RELATIVE` — lock onto a point relative to the firing ship.
    Relative,
    /// `LOCK_TARGET` — hard lock onto a target entity.
    Target,
}

impl WeaponLockType {
    /// Map a raw on-wire value to a `WeaponLockType`. Unrecognized values are
    /// preserved as `Unknown(raw)`.
    pub fn from_raw(raw: u32) -> crate::recognized::Recognized<Self, u32> {
        use crate::recognized::Recognized;
        let known = match raw {
            0 => Self::None,
            1 => Self::Absolute,
            2 => Self::Relative,
            3 => Self::Target,
            _ => return Recognized::Unknown(raw),
        };
        Recognized::Known(known)
    }
}

/// One reason a ship is visible to the opposing team, from the client's
/// `VisionFlags` bit enum.
///
/// The bit positions are read from a modern client build. Bits outside this set
/// are preserved by [`VisibilityFlags::unknown_bits`] rather than dropped, so a
/// build that adds or omits flags still round-trips.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "rkyv", derive(rkyv::Archive, rkyv::Serialize, rkyv::Deserialize))]
#[repr(u32)]
pub enum VisionFlag {
    /// Direct line-of-sight spotting by a ship.
    Ship = 0,
    /// Spotted by a carrier's attacking squadron.
    MainPlane = 1,
    /// Spotted through the always-on short-range x-ray vision every ship has.
    CommonXRay = 2,
    /// Hydroacoustic-style detection visible only to the spotter, not the team.
    RlsPersonal = 3,
    /// Surveillance radar.
    Rls = 4,
    /// Hydroacoustic search.
    Sonar = 5,
    /// Firing from within a smoke screen.
    Smoke = 6,
    /// Spotted by a submarine's sonar ping.
    Pinger = 7,
    /// Spotted by a non-attacking squadron (spotter, fighter, smoke plane).
    MiscPlane = 8,
    /// Spotted by the Submarine Surveillance consumable.
    SubmarineLocator = 9,
    /// Spotted by a reconnaissance squadron.
    Recon = 10,
    /// Spotted by anti-missile systems.
    AntiMissile = 11,
}

impl VisionFlag {
    /// Every flag, in ascending bit order.
    pub const ALL: [VisionFlag; 12] = [
        VisionFlag::Ship,
        VisionFlag::MainPlane,
        VisionFlag::CommonXRay,
        VisionFlag::RlsPersonal,
        VisionFlag::Rls,
        VisionFlag::Sonar,
        VisionFlag::Smoke,
        VisionFlag::Pinger,
        VisionFlag::MiscPlane,
        VisionFlag::SubmarineLocator,
        VisionFlag::Recon,
        VisionFlag::AntiMissile,
    ];

    pub fn bit(self) -> u32 {
        1 << (self as u32)
    }

    /// The client's own identifier for this flag.
    pub fn name(self) -> &'static str {
        match self {
            VisionFlag::Ship => "BY_SHIP",
            VisionFlag::MainPlane => "BY_MAIN_PLANE",
            VisionFlag::CommonXRay => "BY_COMMON_XRAY",
            VisionFlag::RlsPersonal => "BY_RLS_PERSONAL",
            VisionFlag::Rls => "BY_RLS",
            VisionFlag::Sonar => "BY_SONAR",
            VisionFlag::Smoke => "IN_SMOKE",
            VisionFlag::Pinger => "BY_PINGER",
            VisionFlag::MiscPlane => "BY_MISC_PLANE",
            VisionFlag::SubmarineLocator => "BY_SUBMARINE_LOCATOR",
            VisionFlag::Recon => "BY_RECON",
            VisionFlag::AntiMissile => "BY_ANTI_MISSILE",
        }
    }
}

impl fmt::Display for VisionFlag {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.name())
    }
}

/// The `Vehicle.visibilityFlags` bitmask: why this ship is currently visible to
/// the team opposing it. Zero means undetected (the client's `INVISIBLE`).
///
/// The property is `ALL_CLIENTS`, so it is populated for allies and enemies
/// alike. It says how the ship is detected, never by whom.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "serde", serde(transparent))]
#[cfg_attr(feature = "rkyv", derive(rkyv::Archive, rkyv::Serialize, rkyv::Deserialize))]
pub struct VisibilityFlags(u32);

impl VisibilityFlags {
    /// Every bit this table assigns a meaning to.
    const KNOWN: u32 = {
        let mut mask = 0;
        let mut bit = 0;
        while bit < VisionFlag::ALL.len() {
            mask |= 1 << bit;
            bit += 1;
        }
        mask
    };

    pub fn new(raw: u32) -> Self {
        Self(raw)
    }

    pub fn raw(self) -> u32 {
        self.0
    }

    /// Any flag set at all. A ship in smoke that is firing is detected, so this
    /// is true whenever the game considers the ship spotted for any reason.
    pub fn is_detected(self) -> bool {
        self.0 != 0
    }

    pub fn contains(self, flag: VisionFlag) -> bool {
        self.0 & flag.bit() != 0
    }

    /// The recognized flags that are set, in ascending bit order.
    pub fn flags(self) -> impl Iterator<Item = VisionFlag> {
        VisionFlag::ALL.into_iter().filter(move |flag| self.contains(*flag))
    }

    /// Set bits with no known meaning in this build. Non-zero means the client
    /// added a flag this table does not cover yet.
    pub fn unknown_bits(self) -> u32 {
        self.0 & !Self::KNOWN
    }

    /// Spotted by aircraft of any kind.
    pub fn by_any_plane(self) -> bool {
        self.contains(VisionFlag::MainPlane) || self.contains(VisionFlag::MiscPlane) || self.contains(VisionFlag::Recon)
    }

    /// Spotted by radar, whether team-wide or spotter-only.
    pub fn by_any_rls(self) -> bool {
        self.contains(VisionFlag::Rls) || self.contains(VisionFlag::RlsPersonal)
    }

    /// Spotted through a hull-penetrating source rather than line of sight.
    /// Mirrors the client's `BY_XRAY`, which deliberately excludes
    /// `BY_COMMON_XRAY`.
    pub fn by_xray(self) -> bool {
        self.by_any_rls() || self.contains(VisionFlag::Sonar) || self.contains(VisionFlag::SubmarineLocator)
    }

    /// Whether this detection is shared with the spotter's team. Radar marked
    /// spotter-only and submarine pings stay private to whoever made them.
    pub fn visible_for_team(self) -> bool {
        self.0 & !(VisionFlag::RlsPersonal.bit() | VisionFlag::Pinger.bit()) != 0
    }
}

impl fmt::Display for VisibilityFlags {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if !self.is_detected() {
            return write!(f, "INVISIBLE");
        }
        let mut first = true;
        for flag in self.flags() {
            if !first {
                write!(f, "|")?;
            }
            write!(f, "{flag}")?;
            first = false;
        }
        let unknown = self.unknown_bits();
        if unknown != 0 {
            if !first {
                write!(f, "|")?;
            }
            write!(f, "UNKNOWN({unknown:#x})")?;
        }
        Ok(())
    }
}

impl From<u32> for VisibilityFlags {
    fn from(raw: u32) -> Self {
        Self(raw)
    }
}

/// Packed minimap squadron identifier.
/// Encodes `(avatar_id: u32, index: u3, purpose: u3, departures: u1)` in the low 39 bits.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "serde", serde(transparent))]
#[cfg_attr(feature = "rkyv", derive(rkyv::Archive, rkyv::Serialize, rkyv::Deserialize))]
pub struct PlaneId(u64);

impl PlaneId {
    pub fn owner_id(self) -> EntityId {
        EntityId((self.0 & 0xFFFF_FFFF) as u32)
    }

    pub fn raw(self) -> u64 {
        self.0
    }
}

impl fmt::Display for PlaneId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl From<u64> for PlaneId {
    fn from(v: u64) -> Self {
        PlaneId(v)
    }
}

impl From<i64> for PlaneId {
    fn from(v: i64) -> Self {
        PlaneId(v as u64)
    }
}

/// A projectile identifier within a salvo (shell or torpedo).
/// Used to match projectile launches with hit/kill events.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, Hash)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "serde", serde(transparent))]
#[cfg_attr(feature = "rkyv", derive(rkyv::Archive, rkyv::Serialize, rkyv::Deserialize))]
pub struct ShotId(u32);

impl ShotId {
    pub fn raw(self) -> u32 {
        self.0
    }
}

impl fmt::Display for ShotId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl From<u32> for ShotId {
    fn from(v: u32) -> Self {
        ShotId(v)
    }
}

// =============================================================================
// Position Types
// =============================================================================

/// Base 3-component vector. Carries the arithmetic shared by every 3D quantity
/// (positions, velocities, directions). Domain newtypes wrap this and gate which
/// values can be mixed with which.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "rkyv", derive(rkyv::Archive, rkyv::Serialize, rkyv::Deserialize))]
pub struct Vec3 {
    pub x: f32,
    pub y: f32,
    pub z: f32,
}

impl Vec3 {
    pub const fn new(x: f32, y: f32, z: f32) -> Vec3 {
        Vec3 { x, y, z }
    }

    pub fn lerp(self, other: Vec3, t: f32) -> Vec3 {
        self + (other - self) * t
    }
}

impl std::ops::Add for Vec3 {
    type Output = Vec3;
    fn add(self, rhs: Vec3) -> Vec3 {
        Vec3 { x: self.x + rhs.x, y: self.y + rhs.y, z: self.z + rhs.z }
    }
}

impl std::ops::Sub for Vec3 {
    type Output = Vec3;
    fn sub(self, rhs: Vec3) -> Vec3 {
        Vec3 { x: self.x - rhs.x, y: self.y - rhs.y, z: self.z - rhs.z }
    }
}

impl std::ops::Mul<f32> for Vec3 {
    type Output = Vec3;
    fn mul(self, rhs: f32) -> Vec3 {
        Vec3 { x: self.x * rhs, y: self.y * rhs, z: self.z * rhs }
    }
}

impl std::ops::Div<f32> for Vec3 {
    type Output = Vec3;
    fn div(self, rhs: f32) -> Vec3 {
        Vec3 { x: self.x / rhs, y: self.y / rhs, z: self.z / rhs }
    }
}

impl std::iter::Sum for Vec3 {
    fn sum<I: Iterator<Item = Self>>(iter: I) -> Self {
        iter.fold(Vec3::default(), |a, b| Vec3 { x: a.x + b.x, y: a.y + b.y, z: a.z + b.z })
    }
}

impl Vec3 {
    /// Horizontal (XZ-plane) distance to another vector, returned in meters.
    /// Both inputs are in BigWorld coordinates (1 BW = 30m).
    pub fn distance_xz(&self, other: &Vec3) -> Meters {
        let dx = (self.x - other.x) * 30.0;
        let dz = (self.z - other.z) * 30.0;
        Meters::from((dx * dx + dz * dz).sqrt())
    }
}

/// Base 2-component vector. Shared arithmetic for 2D quantities.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "rkyv", derive(rkyv::Archive, rkyv::Serialize, rkyv::Deserialize))]
pub struct Vec2 {
    pub x: f32,
    pub y: f32,
}

impl Vec2 {
    pub const fn new(x: f32, y: f32) -> Vec2 {
        Vec2 { x, y }
    }

    pub fn lerp(self, other: Vec2, t: f32) -> Vec2 {
        self + (other - self) * t
    }
}

impl std::ops::Add for Vec2 {
    type Output = Vec2;
    fn add(self, rhs: Vec2) -> Vec2 {
        Vec2 { x: self.x + rhs.x, y: self.y + rhs.y }
    }
}

impl std::ops::Sub for Vec2 {
    type Output = Vec2;
    fn sub(self, rhs: Vec2) -> Vec2 {
        Vec2 { x: self.x - rhs.x, y: self.y - rhs.y }
    }
}

impl std::ops::Mul<f32> for Vec2 {
    type Output = Vec2;
    fn mul(self, rhs: f32) -> Vec2 {
        Vec2 { x: self.x * rhs, y: self.y * rhs }
    }
}

impl std::ops::Div<f32> for Vec2 {
    type Output = Vec2;
    fn div(self, rhs: f32) -> Vec2 {
        Vec2 { x: self.x / rhs, y: self.y / rhs }
    }
}

impl std::iter::Sum for Vec2 {
    fn sum<I: Iterator<Item = Self>>(iter: I) -> Self {
        iter.fold(Vec2::default(), |a, b| Vec2 { x: a.x + b.x, y: a.y + b.y })
    }
}

/// World-space position in BigWorld coordinates.
/// X = east/west, Y = up/down (altitude), Z = north/south. Origin at map center.
/// Serializes transparently as the inner `Vec3` (emits `x`/`y`/`z`).
#[derive(Debug, Clone, Copy, Default, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize))]
#[cfg_attr(feature = "serde", serde(transparent))]
#[cfg_attr(feature = "rkyv", derive(rkyv::Archive, rkyv::Serialize, rkyv::Deserialize))]
pub struct WorldPos(pub Vec3);

impl WorldPos {
    pub const fn new(x: f32, y: f32, z: f32) -> WorldPos {
        WorldPos(Vec3::new(x, y, z))
    }

    pub fn lerp(self, other: WorldPos, t: f32) -> WorldPos {
        WorldPos(self.0.lerp(other.0, t))
    }
}

impl std::ops::Deref for WorldPos {
    type Target = Vec3;
    fn deref(&self) -> &Vec3 {
        &self.0
    }
}

impl std::ops::DerefMut for WorldPos {
    fn deref_mut(&mut self) -> &mut Vec3 {
        &mut self.0
    }
}

impl std::ops::Add for WorldPos {
    type Output = WorldPos;
    fn add(self, rhs: WorldPos) -> WorldPos {
        WorldPos(self.0 + rhs.0)
    }
}

impl std::ops::Sub for WorldPos {
    type Output = WorldPos;
    fn sub(self, rhs: WorldPos) -> WorldPos {
        WorldPos(self.0 - rhs.0)
    }
}

impl std::ops::Mul<f32> for WorldPos {
    type Output = WorldPos;
    fn mul(self, rhs: f32) -> WorldPos {
        WorldPos(self.0 * rhs)
    }
}

impl std::ops::Div<f32> for WorldPos {
    type Output = WorldPos;
    fn div(self, rhs: f32) -> WorldPos {
        WorldPos(self.0 / rhs)
    }
}

impl std::iter::Sum for WorldPos {
    fn sum<I: Iterator<Item = Self>>(iter: I) -> Self {
        WorldPos(iter.map(|p| p.0).sum())
    }
}

impl WorldPos {
    /// Horizontal (XZ-plane) distance to another position, returned in meters.
    pub fn distance_xz(&self, other: &WorldPos) -> Meters {
        self.0.distance_xz(&other.0)
    }
}

/// Linear velocity in m/s. Distinct from position so the two cannot be mixed.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize))]
#[cfg_attr(feature = "serde", serde(transparent))]
#[cfg_attr(feature = "rkyv", derive(rkyv::Archive, rkyv::Serialize, rkyv::Deserialize))]
pub struct Velocity(pub Vec3);

impl std::ops::Deref for Velocity {
    type Target = Vec3;
    fn deref(&self) -> &Vec3 {
        &self.0
    }
}

/// Angular velocity in rad/s.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize))]
#[cfg_attr(feature = "serde", serde(transparent))]
#[cfg_attr(feature = "rkyv", derive(rkyv::Archive, rkyv::Serialize, rkyv::Deserialize))]
pub struct AngularVelocity(pub Vec3);

impl std::ops::Deref for AngularVelocity {
    type Target = Vec3;
    fn deref(&self) -> &Vec3 {
        &self.0
    }
}

/// A heading/direction vector. Magnitude is domain-specific (e.g. torpedo
/// direction magnitude is the speed in m/s).
#[derive(Debug, Clone, Copy, Default, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize))]
#[cfg_attr(feature = "serde", serde(transparent))]
#[cfg_attr(feature = "rkyv", derive(rkyv::Archive, rkyv::Serialize, rkyv::Deserialize))]
pub struct Direction(pub Vec3);

impl std::ops::Deref for Direction {
    type Target = Vec3;
    fn deref(&self) -> &Vec3 {
        &self.0
    }
}

/// 2D world-space position (X/Z plane) for entities that lack altitude data,
/// such as minimap plane squadron positions.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "rkyv", derive(rkyv::Archive, rkyv::Serialize, rkyv::Deserialize))]
pub struct WorldPos2D {
    pub x: f32,
    pub z: f32,
}

impl WorldPos2D {
    /// Promote to 3D with `y = 0.0`.
    pub fn to_world_pos(self) -> WorldPos {
        WorldPos::new(self.x, 0.0, self.z)
    }
}

/// Normalized minimap position from MinimapUpdate packets.
/// Values roughly in [-0.5, 1.5] range (centered around [0,1]).
/// Serializes transparently as the inner `Vec2`.
#[derive(Debug, Clone, Copy)]
#[cfg_attr(feature = "serde", derive(serde::Serialize))]
#[cfg_attr(feature = "serde", serde(transparent))]
#[cfg_attr(feature = "rkyv", derive(rkyv::Archive, rkyv::Serialize, rkyv::Deserialize))]
pub struct NormalizedPos(pub Vec2);

impl NormalizedPos {
    pub const fn new(x: f32, y: f32) -> NormalizedPos {
        NormalizedPos(Vec2::new(x, y))
    }

    pub fn lerp(self, other: NormalizedPos, t: f32) -> NormalizedPos {
        NormalizedPos(self.0.lerp(other.0, t))
    }
}

impl std::ops::Deref for NormalizedPos {
    type Target = Vec2;
    fn deref(&self) -> &Vec2 {
        &self.0
    }
}

// =============================================================================
// Time Types
// =============================================================================

/// A game clock value in seconds since the replay started recording.
/// Note: there is typically a ~30s pre-game countdown, so game_time = clock - 30.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "rkyv", derive(rkyv::Archive, rkyv::Serialize, rkyv::Deserialize))]
pub struct GameClock(pub f32);

impl GameClock {
    pub fn seconds(self) -> f32 {
        self.0
    }

    pub fn to_duration(self) -> std::time::Duration {
        std::time::Duration::from_secs_f32(self.0)
    }

    pub fn game_time(self) -> f32 {
        (self.0 - 30.0).max(0.0)
    }
}

impl fmt::Display for GameClock {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{:.1}s", self.0)
    }
}

impl Eq for GameClock {}

impl PartialOrd for GameClock {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for GameClock {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        self.0.total_cmp(&other.0)
    }
}

impl std::hash::Hash for GameClock {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        self.0.to_bits().hash(state);
    }
}

impl std::ops::Add<f32> for GameClock {
    type Output = GameClock;
    fn add(self, rhs: f32) -> GameClock {
        GameClock(self.0 + rhs)
    }
}

impl std::ops::Add<std::time::Duration> for GameClock {
    type Output = GameClock;
    fn add(self, rhs: std::time::Duration) -> GameClock {
        GameClock(self.0 + rhs.as_secs_f32())
    }
}

impl std::ops::Sub for GameClock {
    type Output = f32;
    fn sub(self, rhs: GameClock) -> f32 {
        self.0 - rhs.0
    }
}

impl std::ops::Sub<std::time::Duration> for GameClock {
    type Output = GameClock;
    fn sub(self, rhs: std::time::Duration) -> GameClock {
        GameClock(self.0 - rhs.as_secs_f32())
    }
}

impl std::ops::Sub<f32> for GameClock {
    type Output = GameClock;
    fn sub(self, rhs: f32) -> GameClock {
        GameClock(self.0 - rhs)
    }
}

impl From<f32> for GameClock {
    fn from(secs: f32) -> Self {
        GameClock(secs)
    }
}

impl GameClock {
    /// Convert to elapsed time given a battle start epoch.
    pub fn to_elapsed(self, battle_start: GameClock) -> ElapsedClock {
        ElapsedClock((self.0 - battle_start.0).max(0.0))
    }
}

/// Seconds elapsed since battle start (battleStage transition).
/// Distinct from GameClock which counts from replay recording start.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "rkyv", derive(rkyv::Archive, rkyv::Serialize, rkyv::Deserialize))]
pub struct ElapsedClock(pub f32);

impl ElapsedClock {
    pub fn seconds(self) -> f32 {
        self.0
    }

    /// Convert back to absolute GameClock given a battle start epoch.
    pub fn to_absolute(self, battle_start: GameClock) -> GameClock {
        GameClock(battle_start.0 + self.0)
    }
}

impl Eq for ElapsedClock {}

impl PartialOrd for ElapsedClock {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for ElapsedClock {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        self.0.total_cmp(&other.0)
    }
}

impl std::hash::Hash for ElapsedClock {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        self.0.to_bits().hash(state);
    }
}

impl fmt::Display for ElapsedClock {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{:.1}s", self.0)
    }
}

impl std::ops::Add<f32> for ElapsedClock {
    type Output = ElapsedClock;
    fn add(self, rhs: f32) -> ElapsedClock {
        ElapsedClock(self.0 + rhs)
    }
}

impl std::ops::Sub for ElapsedClock {
    type Output = f32;
    fn sub(self, rhs: ElapsedClock) -> f32 {
        self.0 - rhs.0
    }
}

impl std::ops::Sub<f32> for ElapsedClock {
    type Output = ElapsedClock;
    fn sub(self, rhs: f32) -> ElapsedClock {
        ElapsedClock(self.0 - rhs)
    }
}

impl From<f32> for ElapsedClock {
    fn from(secs: f32) -> Self {
        ElapsedClock(secs)
    }
}

// =============================================================================
// Game Event Enums
// =============================================================================

/// Voice line commands sent by players via quick-chat.
#[derive(Debug, Clone, Copy)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "rkyv", derive(rkyv::Archive, rkyv::Serialize, rkyv::Deserialize))]
pub enum VoiceLine {
    IntelRequired,
    FairWinds,
    Wilco,
    Negative,
    WellDone,
    Curses,
    UsingRadar,
    UsingHydroSearch,
    DefendTheBase,
    SetSmokeScreen,
    FollowMe,
    MapPointAttention(f32, f32),
    UsingSubmarineLocator,
    ProvideAntiAircraft,
    RequestingSupport(Option<u32>),
    Retreat(Option<i32>),
    AttentionToSquare(u32, u32),
    Unknown(i64),
    QuickTactic(u16, u64),
}

/// Enumerates the ribbons which appear in the top-right.
#[derive(Debug, PartialEq, Eq, Hash, Clone, Copy, PartialOrd, Ord)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "rkyv", derive(rkyv::Archive, rkyv::Serialize, rkyv::Deserialize))]
pub enum Ribbon {
    PlaneShotDown,
    Incapacitation,
    SetFire,
    Citadel,
    SecondaryHit,
    OverPenetration,
    Penetration,
    NonPenetration,
    Ricochet,
    TorpedoProtectionHit,
    Captured,
    AssistedInCapture,
    Spotted,
    Destroyed,
    TorpedoHit,
    Defended,
    Flooding,
    DiveBombPenetration,
    RocketPenetration,
    RocketNonPenetration,
    RocketTorpedoProtectionHit,
    DepthChargeHit,
    ShotDownByAircraft,
    BuffSeized,
    SonarOneHit,
    SonarTwoHits,
    SonarNeutralized,
    MainCaliber,
    Bomb,
    Suppressed,
    BuildingKill,
    BombOverPenetration,
    BombNonPenetration,
    BombRicochet,
    Rocket,
    BombTorpedoProtectionHit,
    Drop,
    RocketRicochet,
    RocketOverPenetration,
    WaveKillTorpedo,
    WaveCutWave,
    WaveHitVehicle,
    AcousticHitVehicleNew,
    AcousticHitVehicleCurr,
    AcousticHitVehicleBlock,
    Acid,
    DepthChargeFullDamage,
    DepthChargePartialDamage,
    Mine,
    DeminingMine,
    DeminingMinefield,
    TorpedoPhotonHit,
    TorpedoPhotonSplash,
    AimPulseTorpedoPhoton,
    PhaserLaser,
    ShieldHit,
    ShieldRemoved,
    Assist,
    Missile,
    ShotDownMissile,
    Wave,
    TorpedoPhoton,
    Shield,
    Unknown(i8),
}

impl Ribbon {
    /// Returns the player-results key for this ribbon (e.g. `"RIBBON_MAIN_CALIBER_PENETRATION"`).
    ///
    /// This key can be passed to [`translations::translate_ribbon()`] to get localized
    /// display names, descriptions, and icon keys.
    ///
    /// Returns `None` for `Unknown` variants or ribbons without a known results key.
    pub fn translation_key(&self) -> Option<&'static str> {
        match self {
            Ribbon::PlaneShotDown => Some("RIBBON_PLANE"),
            Ribbon::Incapacitation => Some("RIBBON_CRIT"),
            Ribbon::SetFire => Some("RIBBON_BURN"),
            Ribbon::Citadel => Some("RIBBON_CITADEL"),
            Ribbon::SecondaryHit => Some("RIBBON_SECONDARY_CALIBER"),
            Ribbon::OverPenetration => Some("RIBBON_MAIN_CALIBER_OVER_PENETRATION"),
            Ribbon::Penetration => Some("RIBBON_MAIN_CALIBER_PENETRATION"),
            Ribbon::NonPenetration => Some("RIBBON_MAIN_CALIBER_NO_PENETRATION"),
            Ribbon::Ricochet => Some("RIBBON_MAIN_CALIBER_RICOCHET"),
            Ribbon::TorpedoProtectionHit => Some("RIBBON_BULGE"),
            Ribbon::Captured => Some("RIBBON_BASE_CAPTURE"),
            Ribbon::AssistedInCapture => Some("RIBBON_BASE_CAPTURE_ASSIST"),
            Ribbon::Spotted => Some("RIBBON_DETECTED"),
            Ribbon::Destroyed => Some("RIBBON_FRAG"),
            Ribbon::TorpedoHit => Some("RIBBON_TORPEDO"),
            Ribbon::Defended => Some("RIBBON_BASE_DEFENSE"),
            Ribbon::Flooding => Some("RIBBON_FLOOD"),
            Ribbon::DiveBombPenetration => Some("RIBBON_BOMB_PENETRATION"),
            Ribbon::RocketPenetration => Some("RIBBON_ROCKET_PENETRATION"),
            Ribbon::RocketNonPenetration => Some("RIBBON_ROCKET_NO_PENETRATION"),
            Ribbon::RocketTorpedoProtectionHit => Some("RIBBON_ROCKET_BULGE"),
            Ribbon::DepthChargeHit => Some("RIBBON_DBOMB"),
            Ribbon::ShotDownByAircraft => Some("RIBBON_SPLANE"),
            Ribbon::BuffSeized => None, // No known results key
            Ribbon::SonarOneHit => Some("RIBBON_ACOUSTIC_HIT"),
            Ribbon::SonarTwoHits => None,
            Ribbon::SonarNeutralized => None,
            Ribbon::MainCaliber => Some("RIBBON_MAIN_CALIBER"),
            Ribbon::Bomb => Some("RIBBON_BOMB"),
            Ribbon::Suppressed => Some("RIBBON_SUPPRESSED"),
            Ribbon::BuildingKill => Some("RIBBON_BUILDING_KILL"),
            Ribbon::BombOverPenetration => Some("RIBBON_BOMB_OVER_PENETRATION"),
            Ribbon::BombNonPenetration => Some("RIBBON_BOMB_NO_PENETRATION"),
            Ribbon::BombRicochet => Some("RIBBON_BOMB_RICOCHET"),
            Ribbon::Rocket => Some("RIBBON_ROCKET"),
            Ribbon::BombTorpedoProtectionHit => Some("RIBBON_BOMB_BULGE"),
            Ribbon::Drop => Some("RIBBON_DROP"),
            Ribbon::RocketRicochet => Some("RIBBON_ROCKET_RICOCHET"),
            Ribbon::RocketOverPenetration => Some("RIBBON_ROCKET_OVER_PENETRATION"),
            Ribbon::WaveKillTorpedo => Some("RIBBON_WAVE_KILL_TORPEDO"),
            Ribbon::WaveCutWave => Some("RIBBON_WAVE_CUT_WAVE"),
            Ribbon::WaveHitVehicle => Some("RIBBON_WAVE_HIT_VEHICLE"),
            Ribbon::AcousticHitVehicleNew => Some("RIBBON_ACOUSTIC_HIT_VEHICLE_NEW"),
            Ribbon::AcousticHitVehicleCurr => Some("RIBBON_ACOUSTIC_HIT_VEHICLE_CURR"),
            Ribbon::AcousticHitVehicleBlock => Some("RIBBON_ACOUSTIC_HIT_VEHICLE_BLOCK"),
            Ribbon::Acid => Some("RIBBON_ACID"),
            Ribbon::DepthChargeFullDamage => Some("RIBBON_DBOMB_FULL_DAMAGE"),
            Ribbon::DepthChargePartialDamage => Some("RIBBON_DBOMB_PARTIAL_DAMAGE"),
            Ribbon::Mine => Some("RIBBON_MINE"),
            Ribbon::DeminingMine => Some("RIBBON_DEMINING_MINE"),
            Ribbon::DeminingMinefield => Some("RIBBON_DEMINING_MINEFIELD"),
            Ribbon::TorpedoPhotonHit => Some("RIBBON_TORPEDO_PHOTON_HIT"),
            Ribbon::TorpedoPhotonSplash => Some("RIBBON_TORPEDO_PHOTON_SPLASH"),
            Ribbon::AimPulseTorpedoPhoton => Some("RIBBON_AIM_PULSE_TORPEDO_PHOTON"),
            Ribbon::PhaserLaser => Some("RIBBON_PHASER_LASER"),
            Ribbon::ShieldHit => Some("RIBBON_SHIELD_HIT"),
            Ribbon::ShieldRemoved => Some("RIBBON_SHIELD_REMOVED"),
            Ribbon::Assist => Some("RIBBON_ASSIST"),
            Ribbon::Missile => Some("RIBBON_MISSILE"),
            Ribbon::ShotDownMissile => Some("SHOT_DOWN_MISSILE"),
            Ribbon::Wave => Some("RIBBON_WAVE"),
            Ribbon::TorpedoPhoton => Some("RIBBON_TORPEDO_PHOTON"),
            Ribbon::Shield => Some("RIBBON_SHIELD"),
            Ribbon::Unknown(_) => None,
        }
    }

    /// Resolve a numeric ribbon id to a `Ribbon`.
    ///
    /// Covers the full modern id space 0..=59 (source: med80ffdd RibbonsType,
    /// build 12506899). Used by the avatar `privateVehicleState.ribbons` path.
    /// The legacy `onRibbon` RPC uses a different older id space (see decode.rs).
    pub fn from_id(id: i32) -> Ribbon {
        match id {
            0 => Ribbon::MainCaliber,
            1 => Ribbon::TorpedoHit,
            2 => Ribbon::Bomb,
            3 => Ribbon::PlaneShotDown,
            4 => Ribbon::Incapacitation,
            5 => Ribbon::Destroyed,
            6 => Ribbon::SetFire,
            7 => Ribbon::Flooding,
            8 => Ribbon::Citadel,
            9 => Ribbon::Defended,
            10 => Ribbon::Captured,
            11 => Ribbon::AssistedInCapture,
            12 => Ribbon::Suppressed,
            13 => Ribbon::SecondaryHit,
            14 => Ribbon::OverPenetration,
            15 => Ribbon::Penetration,
            16 => Ribbon::NonPenetration,
            17 => Ribbon::Ricochet,
            18 => Ribbon::BuildingKill,
            19 => Ribbon::Spotted,
            20 => Ribbon::BombOverPenetration,
            21 => Ribbon::DiveBombPenetration,
            22 => Ribbon::BombNonPenetration,
            23 => Ribbon::BombRicochet,
            24 => Ribbon::Rocket,
            25 => Ribbon::RocketPenetration,
            26 => Ribbon::RocketNonPenetration,
            27 => Ribbon::ShotDownByAircraft,
            28 => Ribbon::TorpedoProtectionHit,
            29 => Ribbon::BombTorpedoProtectionHit,
            30 => Ribbon::RocketTorpedoProtectionHit,
            31 => Ribbon::DepthChargeHit,
            32 => Ribbon::SonarOneHit,
            33 => Ribbon::Drop,
            34 => Ribbon::RocketRicochet,
            35 => Ribbon::RocketOverPenetration,
            36 => Ribbon::WaveKillTorpedo,
            37 => Ribbon::WaveCutWave,
            38 => Ribbon::WaveHitVehicle,
            39 => Ribbon::AcousticHitVehicleNew,
            40 => Ribbon::AcousticHitVehicleCurr,
            41 => Ribbon::AcousticHitVehicleBlock,
            42 => Ribbon::Acid,
            43 => Ribbon::DepthChargeFullDamage,
            44 => Ribbon::DepthChargePartialDamage,
            45 => Ribbon::Mine,
            46 => Ribbon::DeminingMine,
            47 => Ribbon::DeminingMinefield,
            48 => Ribbon::TorpedoPhotonHit,
            49 => Ribbon::TorpedoPhotonSplash,
            50 => Ribbon::AimPulseTorpedoPhoton,
            51 => Ribbon::PhaserLaser,
            52 => Ribbon::ShieldHit,
            53 => Ribbon::ShieldRemoved,
            54 => Ribbon::Assist,
            55 => Ribbon::Missile,
            56 => Ribbon::ShotDownMissile,
            57 => Ribbon::Wave,
            58 => Ribbon::TorpedoPhoton,
            59 => Ribbon::Shield,
            other => Ribbon::Unknown(other.clamp(i8::MIN as i32, i8::MAX as i32) as i8),
        }
    }
}

/// Cause of a ship's destruction.
#[derive(Debug, PartialEq, Eq, Hash, Clone, Copy)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "rkyv", derive(rkyv::Archive, rkyv::Serialize, rkyv::Deserialize))]
pub enum DeathCause {
    None,
    Artillery,
    Secondaries,
    Torpedo,
    DiveBomber,
    AerialTorpedo,
    Fire,
    Ramming,
    Terrain,
    Flooding,
    Mirror,
    SeaMine,
    Special,
    DepthCharge,
    AerialRocket,
    Detonation,
    Health,
    ApShell,
    HeShell,
    CsShell,
    Fel,
    Portal,
    SkipBombs,
    SectorWave,
    Acid,
    Laser,
    Match,
    Timer,
    AerialDepthCharge,
    Event1,
    Event2,
    Event3,
    Event4,
    Event5,
    Event6,
    Missile,
}

impl DeathCause {
    pub fn from_id(id: i32, constants: &BattleConstants, version: Version) -> Option<Recognized<Self>> {
        constants.death_reason(id).map(|name| Self::from_name(name, version))
    }

    pub fn from_name(name: &str, _version: Version) -> Recognized<Self> {
        match name {
            "NONE" => Recognized::Known(DeathCause::None),
            "ARTILLERY" => Recognized::Known(DeathCause::Artillery),
            "ATBA" => Recognized::Known(DeathCause::Secondaries),
            "TORPEDO" => Recognized::Known(DeathCause::Torpedo),
            "BOMB" => Recognized::Known(DeathCause::DiveBomber),
            "TBOMB" => Recognized::Known(DeathCause::AerialTorpedo),
            "BURNING" => Recognized::Known(DeathCause::Fire),
            "RAM" => Recognized::Known(DeathCause::Ramming),
            "TERRAIN" => Recognized::Known(DeathCause::Terrain),
            "FLOOD" => Recognized::Known(DeathCause::Flooding),
            "MIRROR" => Recognized::Known(DeathCause::Mirror),
            "SEA_MINE" => Recognized::Known(DeathCause::SeaMine),
            "SPECIAL" => Recognized::Known(DeathCause::Special),
            "DBOMB" => Recognized::Known(DeathCause::DepthCharge),
            "ROCKET" => Recognized::Known(DeathCause::AerialRocket),
            "DETONATE" => Recognized::Known(DeathCause::Detonation),
            "HEALTH" => Recognized::Known(DeathCause::Health),
            "AP_SHELL" => Recognized::Known(DeathCause::ApShell),
            "HE_SHELL" => Recognized::Known(DeathCause::HeShell),
            "CS_SHELL" => Recognized::Known(DeathCause::CsShell),
            "FEL" => Recognized::Known(DeathCause::Fel),
            "PORTAL" => Recognized::Known(DeathCause::Portal),
            "SKIP_BOMB" => Recognized::Known(DeathCause::SkipBombs),
            "SECTOR_WAVE" => Recognized::Known(DeathCause::SectorWave),
            "ACID" => Recognized::Known(DeathCause::Acid),
            "LASER" => Recognized::Known(DeathCause::Laser),
            "MATCH" => Recognized::Known(DeathCause::Match),
            "TIMER" => Recognized::Known(DeathCause::Timer),
            "ADBOMB" => Recognized::Known(DeathCause::AerialDepthCharge),
            "EVENT_1" => Recognized::Known(DeathCause::Event1),
            "EVENT_2" => Recognized::Known(DeathCause::Event2),
            "EVENT_3" => Recognized::Known(DeathCause::Event3),
            "EVENT_4" => Recognized::Known(DeathCause::Event4),
            "EVENT_5" => Recognized::Known(DeathCause::Event5),
            "EVENT_6" => Recognized::Known(DeathCause::Event6),
            "MISSILE" => Recognized::Known(DeathCause::Missile),
            other => Recognized::Unknown(other.to_string()),
        }
    }

    pub const fn name(&self) -> &'static str {
        match self {
            DeathCause::None => "NONE",
            DeathCause::Artillery => "ARTILLERY",
            DeathCause::Secondaries => "ATBA",
            DeathCause::Torpedo => "TORPEDO",
            DeathCause::DiveBomber => "BOMB",
            DeathCause::AerialTorpedo => "TBOMB",
            DeathCause::Fire => "BURNING",
            DeathCause::Ramming => "RAM",
            DeathCause::Terrain => "TERRAIN",
            DeathCause::Flooding => "FLOOD",
            DeathCause::Mirror => "MIRROR",
            DeathCause::SeaMine => "SEA_MINE",
            DeathCause::Special => "SPECIAL",
            DeathCause::DepthCharge => "DBOMB",
            DeathCause::AerialRocket => "ROCKET",
            DeathCause::Detonation => "DETONATE",
            DeathCause::Health => "HEALTH",
            DeathCause::ApShell => "AP_SHELL",
            DeathCause::HeShell => "HE_SHELL",
            DeathCause::CsShell => "CS_SHELL",
            DeathCause::Fel => "FEL",
            DeathCause::Portal => "PORTAL",
            DeathCause::SkipBombs => "SKIP_BOMB",
            DeathCause::SectorWave => "SECTOR_WAVE",
            DeathCause::Acid => "ACID",
            DeathCause::Laser => "LASER",
            DeathCause::Match => "MATCH",
            DeathCause::Timer => "TIMER",
            DeathCause::AerialDepthCharge => "ADBOMB",
            DeathCause::Event1 => "EVENT_1",
            DeathCause::Event2 => "EVENT_2",
            DeathCause::Event3 => "EVENT_3",
            DeathCause::Event4 => "EVENT_4",
            DeathCause::Event5 => "EVENT_5",
            DeathCause::Event6 => "EVENT_6",
            DeathCause::Missile => "MISSILE",
        }
    }

    pub fn icon_name(&self) -> Option<&'static str> {
        match self {
            DeathCause::Artillery => Some("icon_frag_main_caliber"),
            DeathCause::Secondaries => Some("icon_frag_atba"),
            DeathCause::Torpedo => Some("icon_frag_torpedo"),
            DeathCause::DiveBomber => Some("icon_frag_bomb"),
            DeathCause::AerialTorpedo => Some("icon_frag_torpedo"),
            DeathCause::Fire => Some("icon_frag_burning"),
            DeathCause::Ramming => Some("icon_frag_ram"),
            DeathCause::Flooding => Some("icon_frag_flood"),
            DeathCause::SeaMine => Some("icon_frag_naval_mine"),
            DeathCause::DepthCharge => Some("icon_frag_depthbomb"),
            DeathCause::AerialRocket => Some("icon_frag_rocket"),
            DeathCause::Detonation => Some("icon_frag_detonate"),
            DeathCause::ApShell => Some("icon_frag_main_caliber"),
            DeathCause::HeShell => Some("icon_frag_main_caliber"),
            DeathCause::CsShell => Some("icon_frag_main_caliber"),
            DeathCause::Fel => Some("icon_frag_fel"),
            DeathCause::Portal => Some("icon_frag_portal"),
            DeathCause::SkipBombs => Some("icon_frag_skip"),
            DeathCause::SectorWave => Some("icon_frag_wave"),
            DeathCause::Acid => Some("icon_frag_acid"),
            DeathCause::Laser => Some("icon_frag_laser"),
            DeathCause::Match => Some("icon_frag_octagon"),
            DeathCause::Timer => Some("icon_timer"),
            DeathCause::AerialDepthCharge => Some("icon_frag_depthbomb"),
            DeathCause::Event1 => Some("icon_frag_fel"),
            DeathCause::Event2 => Some("icon_frag_fel"),
            DeathCause::Event3 => Some("icon_frag_fel"),
            DeathCause::Event4 => Some("icon_frag_fel"),
            DeathCause::Event5 => Some("icon_frag_fel"),
            DeathCause::Event6 => Some("icon_frag_torpedo"),
            _ => Option::None,
        }
    }
}

/// How a consumable was activated (`ConsumableUsageType` from the game scripts).
///
/// Determines the shape of the `CONSUMABLE_USAGE_PARAMS` blob in 15.2+ replays.
/// Serialized by `UsageConverter` in `CommonConsumables/UsageConverter.pyc`.
#[derive(Debug, PartialEq, Clone, Copy)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub enum ConsumableUsageParams {
    /// `NONE` (type 0) — no parameters.
    None,
    /// `DEFAULT` (type 1) — standard activation, no extra data. Format: `<BB>`.
    Default,
    /// `POSITION` (type 2) — activated at a map position (e.g. tactical consumables). Format: `<BBff>`.
    Position(WorldPos2D),
    /// `ENTITY` (type 3) — targeted at a specific entity. Format: `<BBbQ>`.
    Entity { target_type: i8, target_id: u64 },
}

/// Total available charges for a consumable slot.
///
/// `AbilityCategory::num_consumables` uses `-1` to mean "unlimited" (base
/// Damage Control, for instance). [`from_game_params`] converts at the
/// boundary so the sentinel never leaks past this type.
///
/// [`from_game_params`]: ChargeCount::from_game_params
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "rkyv", derive(rkyv::Archive, rkyv::Serialize, rkyv::Deserialize))]
pub enum ChargeCount {
    Unlimited,
    Finite(u32),
}

impl ChargeCount {
    pub fn from_game_params(num_consumables: isize) -> Self {
        if num_consumables < 0 { ChargeCount::Unlimited } else { ChargeCount::Finite(num_consumables as u32) }
    }

    pub fn saturating_sub(self, used: u32) -> Self {
        match self {
            ChargeCount::Unlimited => ChargeCount::Unlimited,
            ChargeCount::Finite(n) => ChargeCount::Finite(n.saturating_sub(used)),
        }
    }

    pub fn saturating_add(self, extra: u32) -> Self {
        match self {
            ChargeCount::Unlimited => ChargeCount::Unlimited,
            ChargeCount::Finite(n) => ChargeCount::Finite(n.saturating_add(extra)),
        }
    }

    pub fn is_unlimited(self) -> bool {
        matches!(self, ChargeCount::Unlimited)
    }

    pub fn finite(self) -> Option<u32> {
        match self {
            ChargeCount::Finite(n) => Some(n),
            ChargeCount::Unlimited => None,
        }
    }
}

/// Consumable ability type, mapped from `consumableType` in GameParams.
#[derive(Debug, PartialEq, Eq, Hash, Clone, Copy)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "rkyv", derive(rkyv::Archive, rkyv::Serialize, rkyv::Deserialize))]
pub enum Consumable {
    DamageControl,
    SpottingAircraft,
    DefensiveAntiAircraft,
    SpeedBoost,
    MainBatteryReloadBooster,
    Smoke,
    RepairParty,
    CatapultFighter,
    HydroacousticSearch,
    TorpedoReloadBooster,
    Radar,
    Trigger1,
    Trigger2,
    Trigger3,
    Trigger4,
    Trigger5,
    Trigger6,
    Invulnerable,
    HealForsage,
    CallFighters,
    RegenerateHealth,
    SubsOxygenRegen,
    SubsWaveGunBoost,
    SubsFourthState,
    DepthCharges,
    Trigger7,
    Trigger8,
    Trigger9,
    Buff,
    BuffsShift,
    CircleWave,
    GoDeep,
    WeaponReloadBooster,
    Hydrophone,
    EnhancedRudders,
    ReserveBattery,
    GroupAuraBuff,
    AffectedBuffAura,
    InvisibilityExtraBuff,
    SubmarineSurveillance,
    PlaneSmokeGenerator,
    Minefield,
    TacticalTrigger1,
    TacticalTrigger2,
    TacticalTrigger3,
    TacticalTrigger4,
    TacticalTrigger5,
    TacticalTrigger6,
    ReconnaissanceSquad,
    SmokePlane,
    TacticalBuff,
    PlaneTrigger1,
    PlaneTrigger2,
    PlaneTrigger3,
    PlaneBuff,
    Any,
    All,
    Special,
}

impl Consumable {
    pub fn from_id(id: i32, constants: &CommonConstants, version: Version) -> Option<Recognized<Self>> {
        constants.consumable_type(id).map(|name| Self::from_consumable_type(name, version))
    }

    pub fn from_consumable_type(s: &str, _version: Version) -> Recognized<Self> {
        match s {
            "crashCrew" => Recognized::Known(Self::DamageControl),
            "scout" => Recognized::Known(Self::SpottingAircraft),
            "airDefenseDisp" => Recognized::Known(Self::DefensiveAntiAircraft),
            "speedBoosters" => Recognized::Known(Self::SpeedBoost),
            "artilleryBoosters" => Recognized::Known(Self::MainBatteryReloadBooster),
            "smokeGenerator" => Recognized::Known(Self::Smoke),
            "regenCrew" => Recognized::Known(Self::RepairParty),
            "fighter" => Recognized::Known(Self::CatapultFighter),
            "sonar" => Recognized::Known(Self::HydroacousticSearch),
            "torpedoReloader" => Recognized::Known(Self::TorpedoReloadBooster),
            "rls" => Recognized::Known(Self::Radar),
            "trigger1" => Recognized::Known(Self::Trigger1),
            "trigger2" => Recognized::Known(Self::Trigger2),
            "trigger3" => Recognized::Known(Self::Trigger3),
            "trigger4" => Recognized::Known(Self::Trigger4),
            "trigger5" => Recognized::Known(Self::Trigger5),
            "trigger6" => Recognized::Known(Self::Trigger6),
            "invulnerable" => Recognized::Known(Self::Invulnerable),
            "healForsage" => Recognized::Known(Self::HealForsage),
            "callFighters" => Recognized::Known(Self::CallFighters),
            "regenerateHealth" => Recognized::Known(Self::RegenerateHealth),
            "subsOxygenRegen" => Recognized::Known(Self::SubsOxygenRegen),
            "subsWaveGunBoost" => Recognized::Known(Self::SubsWaveGunBoost),
            "subsFourthState" => Recognized::Known(Self::SubsFourthState),
            "depthCharges" => Recognized::Known(Self::DepthCharges),
            "trigger7" => Recognized::Known(Self::Trigger7),
            "trigger8" => Recognized::Known(Self::Trigger8),
            "trigger9" => Recognized::Known(Self::Trigger9),
            "buff" => Recognized::Known(Self::Buff),
            "buffsShift" => Recognized::Known(Self::BuffsShift),
            "circleWave" => Recognized::Known(Self::CircleWave),
            "goDeep" => Recognized::Known(Self::GoDeep),
            "weaponReloadBooster" => Recognized::Known(Self::WeaponReloadBooster),
            "hydrophone" => Recognized::Known(Self::Hydrophone),
            "fastRudders" => Recognized::Known(Self::EnhancedRudders),
            "subsEnergyFreeze" => Recognized::Known(Self::ReserveBattery),
            "groupAuraBuff" => Recognized::Known(Self::GroupAuraBuff),
            "affectedBuffAura" => Recognized::Known(Self::AffectedBuffAura),
            "invisibilityExtraBuffConsumable" => Recognized::Known(Self::InvisibilityExtraBuff),
            "submarineLocator" => Recognized::Known(Self::SubmarineSurveillance),
            "planeSmokeGenerator" => Recognized::Known(Self::PlaneSmokeGenerator),
            "minefield" => Recognized::Known(Self::Minefield),
            "tacticalTrigger1" => Recognized::Known(Self::TacticalTrigger1),
            "tacticalTrigger2" => Recognized::Known(Self::TacticalTrigger2),
            "tacticalTrigger3" => Recognized::Known(Self::TacticalTrigger3),
            "tacticalTrigger4" => Recognized::Known(Self::TacticalTrigger4),
            "tacticalTrigger5" => Recognized::Known(Self::TacticalTrigger5),
            "tacticalTrigger6" => Recognized::Known(Self::TacticalTrigger6),
            "reconnaissanceSquad" => Recognized::Known(Self::ReconnaissanceSquad),
            "smokePlane" => Recognized::Known(Self::SmokePlane),
            "tacticalBuff" => Recognized::Known(Self::TacticalBuff),
            "planeTrigger1" => Recognized::Known(Self::PlaneTrigger1),
            "planeTrigger2" => Recognized::Known(Self::PlaneTrigger2),
            "planeTrigger3" => Recognized::Known(Self::PlaneTrigger3),
            "planeBuff" => Recognized::Known(Self::PlaneBuff),
            "Any" => Recognized::Known(Self::Any),
            "All" => Recognized::Known(Self::All),
            "Special" => Recognized::Known(Self::Special),
            other => Recognized::Unknown(other.to_string()),
        }
    }

    pub const fn name(&self) -> &'static str {
        match self {
            Self::DamageControl => "crashCrew",
            Self::SpottingAircraft => "scout",
            Self::DefensiveAntiAircraft => "airDefenseDisp",
            Self::SpeedBoost => "speedBoosters",
            Self::MainBatteryReloadBooster => "artilleryBoosters",
            Self::Smoke => "smokeGenerator",
            Self::RepairParty => "regenCrew",
            Self::CatapultFighter => "fighter",
            Self::HydroacousticSearch => "sonar",
            Self::TorpedoReloadBooster => "torpedoReloader",
            Self::Radar => "rls",
            Self::Trigger1 => "trigger1",
            Self::Trigger2 => "trigger2",
            Self::Trigger3 => "trigger3",
            Self::Trigger4 => "trigger4",
            Self::Trigger5 => "trigger5",
            Self::Trigger6 => "trigger6",
            Self::Invulnerable => "invulnerable",
            Self::HealForsage => "healForsage",
            Self::CallFighters => "callFighters",
            Self::RegenerateHealth => "regenerateHealth",
            Self::SubsOxygenRegen => "subsOxygenRegen",
            Self::SubsWaveGunBoost => "subsWaveGunBoost",
            Self::SubsFourthState => "subsFourthState",
            Self::DepthCharges => "depthCharges",
            Self::Trigger7 => "trigger7",
            Self::Trigger8 => "trigger8",
            Self::Trigger9 => "trigger9",
            Self::Buff => "buff",
            Self::BuffsShift => "buffsShift",
            Self::CircleWave => "circleWave",
            Self::GoDeep => "goDeep",
            Self::WeaponReloadBooster => "weaponReloadBooster",
            Self::Hydrophone => "hydrophone",
            Self::EnhancedRudders => "fastRudders",
            Self::ReserveBattery => "subsEnergyFreeze",
            Self::GroupAuraBuff => "groupAuraBuff",
            Self::AffectedBuffAura => "affectedBuffAura",
            Self::InvisibilityExtraBuff => "invisibilityExtraBuffConsumable",
            Self::SubmarineSurveillance => "submarineLocator",
            Self::PlaneSmokeGenerator => "planeSmokeGenerator",
            Self::Minefield => "minefield",
            Self::TacticalTrigger1 => "tacticalTrigger1",
            Self::TacticalTrigger2 => "tacticalTrigger2",
            Self::TacticalTrigger3 => "tacticalTrigger3",
            Self::TacticalTrigger4 => "tacticalTrigger4",
            Self::TacticalTrigger5 => "tacticalTrigger5",
            Self::TacticalTrigger6 => "tacticalTrigger6",
            Self::ReconnaissanceSquad => "reconnaissanceSquad",
            Self::SmokePlane => "smokePlane",
            Self::TacticalBuff => "tacticalBuff",
            Self::PlaneTrigger1 => "planeTrigger1",
            Self::PlaneTrigger2 => "planeTrigger2",
            Self::PlaneTrigger3 => "planeTrigger3",
            Self::PlaneBuff => "planeBuff",
            Self::Any => "Any",
            Self::All => "All",
            Self::Special => "Special",
        }
    }
}

/// Camera view mode, from `CAMERA_MODES` in game constants.
#[derive(Debug, Clone, Copy)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "rkyv", derive(rkyv::Archive, rkyv::Serialize, rkyv::Deserialize))]
pub enum CameraMode {
    Airplanes,
    Dock,
    OverheadMap,
    DevFree,
    FollowingShells,
    FollowingPlanes,
    DockModule,
    FollowingShip,
    FreeFlying,
    ReplayFpc,
    FollowingSubmarine,
    TacticalConsumables,
    RespawnMap,
    DockFlags,
    DockEnsign,
    DockLootbox,
    DockNavalFlag,
    IdleGame,
}

impl CameraMode {
    pub fn from_id(id: i32, constants: &BattleConstants, version: Version) -> Option<Recognized<Self>> {
        constants.camera_mode(id).map(|name| Self::from_name(name, version))
    }

    pub fn from_name(name: &str, _version: Version) -> Recognized<Self> {
        match name {
            "AIRPLANES" => Recognized::Known(CameraMode::Airplanes),
            "DOCK" => Recognized::Known(CameraMode::Dock),
            "TACTICALMAP" => Recognized::Known(CameraMode::OverheadMap),
            "DEVFREE" => Recognized::Known(CameraMode::DevFree),
            "SHELLTRACKER" => Recognized::Known(CameraMode::FollowingShells),
            "PLANETRACKER" => Recognized::Known(CameraMode::FollowingPlanes),
            "DOCKMODULE" => Recognized::Known(CameraMode::DockModule),
            "SNAKETAIL" => Recognized::Known(CameraMode::FollowingShip),
            "SPECTATOR" => Recognized::Known(CameraMode::FreeFlying),
            "REPLAY_FPC" => Recognized::Known(CameraMode::ReplayFpc),
            "UNDERWATER" => Recognized::Known(CameraMode::FollowingSubmarine),
            "TACTICAL_CONSUMABLES" => Recognized::Known(CameraMode::TacticalConsumables),
            "RESPAWN_MAP" => Recognized::Known(CameraMode::RespawnMap),
            "DOCKFLAGS" => Recognized::Known(CameraMode::DockFlags),
            "DOCKENSIGN" => Recognized::Known(CameraMode::DockEnsign),
            "DOCKLOOTBOX" => Recognized::Known(CameraMode::DockLootbox),
            "DOCKNAVALFLAG" => Recognized::Known(CameraMode::DockNavalFlag),
            "IDLEGAME" => Recognized::Known(CameraMode::IdleGame),
            other => Recognized::Unknown(other.to_string()),
        }
    }

    pub const fn name(&self) -> &'static str {
        match self {
            CameraMode::Airplanes => "AIRPLANES",
            CameraMode::Dock => "DOCK",
            CameraMode::OverheadMap => "TACTICALMAP",
            CameraMode::DevFree => "DEVFREE",
            CameraMode::FollowingShells => "SHELLTRACKER",
            CameraMode::FollowingPlanes => "PLANETRACKER",
            CameraMode::DockModule => "DOCKMODULE",
            CameraMode::FollowingShip => "SNAKETAIL",
            CameraMode::FreeFlying => "SPECTATOR",
            CameraMode::ReplayFpc => "REPLAY_FPC",
            CameraMode::FollowingSubmarine => "UNDERWATER",
            CameraMode::TacticalConsumables => "TACTICAL_CONSUMABLES",
            CameraMode::RespawnMap => "RESPAWN_MAP",
            CameraMode::DockFlags => "DOCKFLAGS",
            CameraMode::DockEnsign => "DOCKENSIGN",
            CameraMode::DockLootbox => "DOCKLOOTBOX",
            CameraMode::DockNavalFlag => "DOCKNAVALFLAG",
            CameraMode::IdleGame => "IDLEGAME",
        }
    }
}

/// What stage a battle is in
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "rkyv", derive(rkyv::Archive, rkyv::Serialize, rkyv::Deserialize))]
pub enum BattleStage {
    Waiting,
    Battle,
    Ended,
    Results,
    Finishing,
}

impl BattleStage {
    pub fn is_not_started(&self) -> bool {
        matches!(self, Self::Waiting)
    }

    pub fn is_not_ended(&self) -> bool {
        matches!(self, Self::Waiting | Self::Battle | Self::Results | Self::Finishing)
    }

    pub fn is_in_battle(&self) -> bool {
        matches!(self, Self::Battle | Self::Results)
    }

    pub fn is_not_finished(&self) -> bool {
        matches!(self, Self::Waiting | Self::Battle | Self::Results)
    }

    pub fn is_without_results(&self) -> bool {
        matches!(self, Self::Waiting | Self::Battle)
    }
}

impl BattleStage {
    pub fn from_name(name: &str, _version: Version) -> Recognized<Self> {
        match name {
            "WAITING" => Recognized::Known(Self::Waiting),
            "BATTLE" => Recognized::Known(Self::Battle),
            "RESULTS" => Recognized::Known(Self::Results),
            "FINISHING" => Recognized::Known(Self::Finishing),
            "ENDED" => Recognized::Known(Self::Ended),
            other => Recognized::Unknown(other.to_string()),
        }
    }

    pub const fn name(&self) -> &'static str {
        match self {
            BattleStage::Waiting => "WAITING",
            BattleStage::Battle => "BATTLE",
            BattleStage::Results => "RESULTS",
            BattleStage::Finishing => "FINISHING",
            BattleStage::Ended => "ENDED",
        }
    }
}

/// How the battle ended, from `FINISH_TYPE` in battle.xml.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "rkyv", derive(rkyv::Archive, rkyv::Serialize, rkyv::Deserialize))]
pub enum FinishType {
    Unknown,
    Extermination,
    BaseCaptured,
    Timeout,
    Failure,
    Technical,
    Score,
    ScoreOnTimeout,
    PveMainTaskSucceeded,
    PveMainTaskFailed,
    ScoreZero,
    ScoreExcess,
}

impl FinishType {
    pub const fn name(&self) -> &'static str {
        match self {
            FinishType::Unknown => "UNKNOWN",
            FinishType::Extermination => "EXTERMINATION",
            FinishType::BaseCaptured => "BASE",
            FinishType::Timeout => "TIMEOUT",
            FinishType::Failure => "FAILURE",
            FinishType::Technical => "TECHNICAL",
            FinishType::Score => "SCORE",
            FinishType::ScoreOnTimeout => "SCORE_ON_TIMEOUT",
            FinishType::PveMainTaskSucceeded => "PVE_MAIN_TASK_SUCCEEDED",
            FinishType::PveMainTaskFailed => "PVE_MAIN_TASK_FAILED",
            FinishType::ScoreZero => "SCORE_ZERO",
            FinishType::ScoreExcess => "SCORE_EXCESS",
        }
    }

    pub const fn description(&self) -> &'static str {
        match self {
            FinishType::Unknown => "Unknown",
            FinishType::Extermination => "Extermination",
            FinishType::BaseCaptured => "Base Captured",
            FinishType::Timeout => "Timeout",
            FinishType::Failure => "Failure",
            FinishType::Technical => "Technical",
            FinishType::Score => "Score",
            FinishType::ScoreOnTimeout => "Score on Timeout",
            FinishType::PveMainTaskSucceeded => "PvE Main Task Succeeded",
            FinishType::PveMainTaskFailed => "PvE Main Task Failed",
            FinishType::ScoreZero => "Score Zero",
            FinishType::ScoreExcess => "Score Excess",
        }
    }
}

impl FinishType {
    pub fn from_id(id: i32, constants: &BattleConstants, version: Version) -> Option<Recognized<Self>> {
        constants.finish_type(id).map(|name| Self::from_name(name, version))
    }

    pub fn from_name(name: &str, _version: Version) -> Recognized<Self> {
        match name {
            "UNKNOWN" => Recognized::Known(FinishType::Unknown),
            "EXTERMINATION" => Recognized::Known(FinishType::Extermination),
            "BASE" => Recognized::Known(FinishType::BaseCaptured),
            "TIMEOUT" => Recognized::Known(FinishType::Timeout),
            "FAILURE" => Recognized::Known(FinishType::Failure),
            "TECHNICAL" => Recognized::Known(FinishType::Technical),
            "SCORE" => Recognized::Known(FinishType::Score),
            "SCORE_ON_TIMEOUT" => Recognized::Known(FinishType::ScoreOnTimeout),
            "PVE_MAIN_TASK_SUCCEEDED" => Recognized::Known(FinishType::PveMainTaskSucceeded),
            "PVE_MAIN_TASK_FAILED" => Recognized::Known(FinishType::PveMainTaskFailed),
            "SCORE_ZERO" => Recognized::Known(FinishType::ScoreZero),
            "SCORE_EXCESS" => Recognized::Known(FinishType::ScoreExcess),
            other => Recognized::Unknown(other.to_string()),
        }
    }
}

impl fmt::Display for FinishType {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.description())
    }
}

/// Outcome of a battle for a team.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "rkyv", derive(rkyv::Archive, rkyv::Serialize, rkyv::Deserialize))]
pub enum BattleResult {
    Victory,
    Defeat,
    Draw,
}

/// Strength of one team's advantage over the other.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "rkyv", derive(rkyv::Archive, rkyv::Serialize, rkyv::Deserialize))]
pub enum AdvantageLevel {
    Absolute,
    Strong,
    Moderate,
    Weak,
}

impl AdvantageLevel {
    pub fn label(&self) -> &'static str {
        match self {
            AdvantageLevel::Absolute => "Absolute",
            AdvantageLevel::Strong => "Strong",
            AdvantageLevel::Moderate => "Moderate",
            AdvantageLevel::Weak => "Weak",
        }
    }
}

/// Submarine depth state, from `DEPTH_STATE` in battle.xml.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "rkyv", derive(rkyv::Archive, rkyv::Serialize, rkyv::Deserialize))]
#[derive(Default)]
pub enum BuoyancyState {
    Invalid,
    #[default]
    Surface,
    Periscope,
    SemiDeepWater,
    DeepWater,
    DeepWaterInvul,
}

impl BuoyancyState {
    pub const fn name(&self) -> &'static str {
        match self {
            BuoyancyState::Invalid => "INVALID_STATE",
            BuoyancyState::Surface => "SURFACE",
            BuoyancyState::Periscope => "PERISCOPE",
            BuoyancyState::SemiDeepWater => "SEMI_DEEP_WATER",
            BuoyancyState::DeepWater => "DEEP_WATER",
            BuoyancyState::DeepWaterInvul => "DEEP_WATER_INVUL",
        }
    }

    pub const fn description(&self) -> &'static str {
        match self {
            BuoyancyState::Invalid => "Invalid",
            BuoyancyState::Surface => "Surface",
            BuoyancyState::Periscope => "Periscope",
            BuoyancyState::SemiDeepWater => "Semi-Deep",
            BuoyancyState::DeepWater => "Deep",
            BuoyancyState::DeepWaterInvul => "Deep (Invul)",
        }
    }
}

impl BuoyancyState {
    pub fn from_id(id: i32, constants: &BattleConstants, version: Version) -> Option<Recognized<Self>> {
        constants.depth_state(id).map(|name| Self::from_name(name, version))
    }

    pub fn from_name(name: &str, _version: Version) -> Recognized<Self> {
        match name {
            "INVALID_STATE" => Recognized::Known(BuoyancyState::Invalid),
            "SURFACE" => Recognized::Known(BuoyancyState::Surface),
            "PERISCOPE" => Recognized::Known(BuoyancyState::Periscope),
            "SEMI_DEEP_WATER" => Recognized::Known(BuoyancyState::SemiDeepWater),
            "DEEP_WATER" => Recognized::Known(BuoyancyState::DeepWater),
            "DEEP_WATER_INVUL" => Recognized::Known(BuoyancyState::DeepWaterInvul),
            // Legacy names from old battle.xml
            "WORKING" => Recognized::Known(BuoyancyState::SemiDeepWater),
            "INVULNERABLE" => Recognized::Known(BuoyancyState::DeepWaterInvul),
            other => Recognized::Unknown(other.to_string()),
        }
    }
}

impl fmt::Display for BuoyancyState {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.description())
    }
}

/// Selected weapon type, from `SHIP_WEAPON_TYPES` in ships.xml.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "rkyv", derive(rkyv::Archive, rkyv::Serialize, rkyv::Deserialize))]
#[derive(Default)]
pub enum WeaponType {
    #[default]
    Artillery,
    Secondaries,
    Torpedoes,
    Planes,
    Pinger,
}

impl WeaponType {
    pub const fn name(&self) -> &'static str {
        match self {
            WeaponType::Artillery => "ARTILLERY",
            WeaponType::Secondaries => "ATBA",
            WeaponType::Torpedoes => "TORPEDO",
            WeaponType::Planes => "AIRPLANES",
            WeaponType::Pinger => "PINGER",
        }
    }

    pub const fn description(&self) -> &'static str {
        match self {
            WeaponType::Artillery => "Main Battery",
            WeaponType::Secondaries => "Secondaries",
            WeaponType::Torpedoes => "Torpedoes",
            WeaponType::Planes => "Planes",
            WeaponType::Pinger => "Sonar",
        }
    }

    /// Map a raw value from the client's integer `WeaponType` enum
    /// (`scripts/WeaponType.pyc`, e.g. in the `onSetWeaponLock` packet) to a
    /// variant. That enum is wider than the selectable weapons modeled here;
    /// non-selectable types (air defense, depth charges, lasers, waves, air
    /// support, missiles, squadron) and the -1 `NONE` sentinel are preserved as
    /// `Unknown(raw)`.
    pub fn from_raw(raw: u32) -> crate::recognized::Recognized<Self, u32> {
        use crate::recognized::Recognized;
        let known = match raw as i32 {
            0 => Self::Artillery,
            1 => Self::Secondaries,
            2 => Self::Torpedoes,
            3 => Self::Planes,
            6 => Self::Pinger,
            _ => return Recognized::Unknown(raw),
        };
        Recognized::Known(known)
    }
}

impl WeaponType {
    pub fn from_id(id: i32, constants: &ShipsConstants, version: Version) -> Option<Recognized<Self>> {
        constants.weapon_type(id).map(|name| Self::from_name(name, version))
    }

    pub fn from_name(name: &str, _version: Version) -> Recognized<Self> {
        match name {
            "ARTILLERY" => Recognized::Known(WeaponType::Artillery),
            "ATBA" => Recognized::Known(WeaponType::Secondaries),
            "TORPEDO" => Recognized::Known(WeaponType::Torpedoes),
            "AIRPLANES" => Recognized::Known(WeaponType::Planes),
            "PINGER" => Recognized::Known(WeaponType::Pinger),
            other => Recognized::Unknown(other.to_string()),
        }
    }
}

impl fmt::Display for WeaponType {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.description())
    }
}

/// Submarine battery state, from `BATTERY_STATE` in battle.xml.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "rkyv", derive(rkyv::Archive, rkyv::Serialize, rkyv::Deserialize))]
#[derive(Default)]
pub enum BatteryState {
    #[default]
    Idle,
    Charging,
    Discharging,
    CriticalDischarging,
    BrokenCharging,
    BrokenIdle,
    Regeneration,
    Empty,
}

impl BatteryState {
    pub const fn name(&self) -> &'static str {
        match self {
            BatteryState::Idle => "IDLE",
            BatteryState::Charging => "CHARGING",
            BatteryState::Discharging => "DISCHARGING",
            BatteryState::CriticalDischarging => "CRITICAL_DISCHARGING",
            BatteryState::BrokenCharging => "BROKEN_CHARGING",
            BatteryState::BrokenIdle => "BROKEN_IDLE",
            BatteryState::Regeneration => "REGENERATION",
            BatteryState::Empty => "EMPTY",
        }
    }

    pub const fn description(&self) -> &'static str {
        match self {
            BatteryState::Idle => "Idle",
            BatteryState::Charging => "Charging",
            BatteryState::Discharging => "Discharging",
            BatteryState::CriticalDischarging => "Critical Discharging",
            BatteryState::BrokenCharging => "Broken Charging",
            BatteryState::BrokenIdle => "Broken Idle",
            BatteryState::Regeneration => "Regeneration",
            BatteryState::Empty => "Empty",
        }
    }
}

impl BatteryState {
    pub fn from_id(id: i32, constants: &BattleConstants, version: Version) -> Option<Recognized<Self>> {
        constants.battery_state(id).map(|name| Self::from_name(name, version))
    }

    pub fn from_name(name: &str, _version: Version) -> Recognized<Self> {
        match name {
            "IDLE" => Recognized::Known(BatteryState::Idle),
            "CHARGING" => Recognized::Known(BatteryState::Charging),
            "DISCHARGING" => Recognized::Known(BatteryState::Discharging),
            "CRITICAL_DISCHARGING" => Recognized::Known(BatteryState::CriticalDischarging),
            "BROKEN_CHARGING" => Recognized::Known(BatteryState::BrokenCharging),
            "BROKEN_IDLE" => Recognized::Known(BatteryState::BrokenIdle),
            "REGENERATION" => Recognized::Known(BatteryState::Regeneration),
            "EMPTY" => Recognized::Known(BatteryState::Empty),
            other => Recognized::Unknown(other.to_string()),
        }
    }
}

impl fmt::Display for BatteryState {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.description())
    }
}

/// Battle type, mapped from `gameType` in replay metadata.
/// Values come from the BATTLE_TYPES enum in `gui/data/constants/common.xml`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "rkyv", derive(rkyv::Archive, rkyv::Serialize, rkyv::Deserialize))]
pub enum BattleType {
    Standard,
    Single,
    Study,
    Random,
    Training,
    Cooperative,
    Ranked,
    OldRanked,
    IntroMission,
    Club,
    Pve,
    Clan,
    Event,
    Brawl,
}

impl BattleType {
    /// Whether this battle type uses full-team divisions (no individual div coloring).
    pub fn is_clan_battle(&self) -> bool {
        matches!(self, Self::Clan)
    }

    pub const fn name(&self) -> &'static str {
        match self {
            Self::Standard => "StandartBattle",
            Self::Single => "SingleBattle",
            Self::Study => "Study",
            Self::Random => "RandomBattle",
            Self::Training => "TrainingBattle",
            Self::Cooperative => "CooperativeBattle",
            Self::Ranked => "RankedBattle",
            Self::OldRanked => "OldRankedBattle",
            Self::IntroMission => "TutorialBattle",
            Self::Club => "ClubBattle",
            Self::Pve => "PVEBattle",
            Self::Clan => "ClanBattle",
            Self::Event => "EventBattle",
            Self::Brawl => "BrawlBattle",
        }
    }
}

impl BattleType {
    /// Parse from the string value in replay metadata (e.g. `"RandomBattle"`).
    pub fn from_value(s: &str, _version: Version) -> Recognized<Self> {
        match s {
            "StandartBattle" => Recognized::Known(Self::Standard),
            "SingleBattle" => Recognized::Known(Self::Single),
            "Study" => Recognized::Known(Self::Study),
            "RandomBattle" => Recognized::Known(Self::Random),
            "TrainingBattle" => Recognized::Known(Self::Training),
            "CooperativeBattle" => Recognized::Known(Self::Cooperative),
            "RankedBattle" => Recognized::Known(Self::Ranked),
            "OldRankedBattle" => Recognized::Known(Self::OldRanked),
            "TutorialBattle" => Recognized::Known(Self::IntroMission),
            "ClubBattle" => Recognized::Known(Self::Club),
            "PVEBattle" => Recognized::Known(Self::Pve),
            "ClanBattle" => Recognized::Known(Self::Clan),
            "EventBattle" => Recognized::Known(Self::Event),
            "BrawlBattle" => Recognized::Known(Self::Brawl),
            other => Recognized::Unknown(other.to_string()),
        }
    }
}

impl fmt::Display for BattleType {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.name())
    }
}

/// Game mode, mapped from the `GAME_MODE` class in `shared_constants.py`.
/// The ids are the game's own and have gaps at 3 through 6; `Invalid` is a
/// real value the game uses, not an error case.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "rkyv", derive(rkyv::Archive, rkyv::Serialize, rkyv::Deserialize))]
pub enum GameMode {
    Invalid,
    Test,
    Standart,
    Singlebase,
    Domination,
    Tutorial,
    Megabase,
    Forts,
    StandardDomination,
    Epicenter,
    AssaultDefense,
    Pve,
    ArmsRace,
    EpicenterRing,
    AntiStandard,
    AttackDefense,
    TorpedoBeat,
    TeamBattleRoyale,
    EscapeToPortal,
    DominationAsymm,
    KeyBattle,
    Portal2021,
    TeamBattleRoyale2021,
    ConvoyEvent,
    ConvoyAirship,
    TwoTeamsBattleRoyale,
    PinataEvent,
    Respawns,
    RespawnsSectors,
}

impl GameMode {
    pub const ALL: [GameMode; 29] = [
        GameMode::Invalid,
        GameMode::Test,
        GameMode::Standart,
        GameMode::Singlebase,
        GameMode::Domination,
        GameMode::Tutorial,
        GameMode::Megabase,
        GameMode::Forts,
        GameMode::StandardDomination,
        GameMode::Epicenter,
        GameMode::AssaultDefense,
        GameMode::Pve,
        GameMode::ArmsRace,
        GameMode::EpicenterRing,
        GameMode::AntiStandard,
        GameMode::AttackDefense,
        GameMode::TorpedoBeat,
        GameMode::TeamBattleRoyale,
        GameMode::EscapeToPortal,
        GameMode::DominationAsymm,
        GameMode::KeyBattle,
        GameMode::Portal2021,
        GameMode::TeamBattleRoyale2021,
        GameMode::ConvoyEvent,
        GameMode::ConvoyAirship,
        GameMode::TwoTeamsBattleRoyale,
        GameMode::PinataEvent,
        GameMode::Respawns,
        GameMode::RespawnsSectors,
    ];

    pub const fn id(self) -> i32 {
        match self {
            GameMode::Invalid => -1,
            GameMode::Test => 0,
            GameMode::Standart => 1,
            GameMode::Singlebase => 2,
            GameMode::Domination => 7,
            GameMode::Tutorial => 8,
            GameMode::Megabase => 9,
            GameMode::Forts => 10,
            GameMode::StandardDomination => 11,
            GameMode::Epicenter => 12,
            GameMode::AssaultDefense => 13,
            GameMode::Pve => 14,
            GameMode::ArmsRace => 15,
            GameMode::EpicenterRing => 16,
            GameMode::AntiStandard => 17,
            GameMode::AttackDefense => 18,
            GameMode::TorpedoBeat => 19,
            GameMode::TeamBattleRoyale => 20,
            GameMode::EscapeToPortal => 21,
            GameMode::DominationAsymm => 22,
            GameMode::KeyBattle => 23,
            GameMode::Portal2021 => 24,
            GameMode::TeamBattleRoyale2021 => 25,
            GameMode::ConvoyEvent => 26,
            GameMode::ConvoyAirship => 27,
            GameMode::TwoTeamsBattleRoyale => 28,
            GameMode::PinataEvent => 29,
            GameMode::Respawns => 30,
            GameMode::RespawnsSectors => 31,
        }
    }

    /// Whether this mode's id fits `ReplayMeta.gameMode`'s wire type, `u32`.
    /// `Invalid` is the one variant with a negative id (-1, the game's own
    /// sentinel for "no mode"), which a `u32` field can never carry; the
    /// indexer (`replay_index.rs`) can therefore never write it into
    /// `indexed_match.game_mode_id`. A filter or dropdown built from
    /// `game_mode_id`'s value space must exclude exactly the modes this
    /// returns `false` for, or it offers a choice the column can never
    /// satisfy.
    pub const fn is_offerable(self) -> bool {
        self.id() >= 0
    }

    pub fn from_id(id: i32) -> Recognized<GameMode, i32> {
        match id {
            -1 => Recognized::Known(GameMode::Invalid),
            0 => Recognized::Known(GameMode::Test),
            1 => Recognized::Known(GameMode::Standart),
            2 => Recognized::Known(GameMode::Singlebase),
            7 => Recognized::Known(GameMode::Domination),
            8 => Recognized::Known(GameMode::Tutorial),
            9 => Recognized::Known(GameMode::Megabase),
            10 => Recognized::Known(GameMode::Forts),
            11 => Recognized::Known(GameMode::StandardDomination),
            12 => Recognized::Known(GameMode::Epicenter),
            13 => Recognized::Known(GameMode::AssaultDefense),
            14 => Recognized::Known(GameMode::Pve),
            15 => Recognized::Known(GameMode::ArmsRace),
            16 => Recognized::Known(GameMode::EpicenterRing),
            17 => Recognized::Known(GameMode::AntiStandard),
            18 => Recognized::Known(GameMode::AttackDefense),
            19 => Recognized::Known(GameMode::TorpedoBeat),
            20 => Recognized::Known(GameMode::TeamBattleRoyale),
            21 => Recognized::Known(GameMode::EscapeToPortal),
            22 => Recognized::Known(GameMode::DominationAsymm),
            23 => Recognized::Known(GameMode::KeyBattle),
            24 => Recognized::Known(GameMode::Portal2021),
            25 => Recognized::Known(GameMode::TeamBattleRoyale2021),
            26 => Recognized::Known(GameMode::ConvoyEvent),
            27 => Recognized::Known(GameMode::ConvoyAirship),
            28 => Recognized::Known(GameMode::TwoTeamsBattleRoyale),
            29 => Recognized::Known(GameMode::PinataEvent),
            30 => Recognized::Known(GameMode::Respawns),
            31 => Recognized::Known(GameMode::RespawnsSectors),
            other => Recognized::Unknown(other),
        }
    }

    /// Lowercase kebab-case token used in the query grammar.
    pub const fn as_token(self) -> &'static str {
        match self {
            GameMode::Invalid => "invalid",
            GameMode::Test => "test",
            GameMode::Standart => "standard",
            GameMode::Singlebase => "singlebase",
            GameMode::Domination => "domination",
            GameMode::Tutorial => "tutorial",
            GameMode::Megabase => "megabase",
            GameMode::Forts => "forts",
            GameMode::StandardDomination => "standard-domination",
            GameMode::Epicenter => "epicenter",
            GameMode::AssaultDefense => "assault-defense",
            GameMode::Pve => "pve",
            GameMode::ArmsRace => "arms-race",
            GameMode::EpicenterRing => "epicenter-ring",
            GameMode::AntiStandard => "anti-standard",
            GameMode::AttackDefense => "attack-defense",
            GameMode::TorpedoBeat => "torpedo-beat",
            GameMode::TeamBattleRoyale => "team-battle-royale",
            GameMode::EscapeToPortal => "escape-to-portal",
            GameMode::DominationAsymm => "domination-asymm",
            GameMode::KeyBattle => "key-battle",
            GameMode::Portal2021 => "portal-2021",
            GameMode::TeamBattleRoyale2021 => "team-battle-royale-2021",
            GameMode::ConvoyEvent => "convoy-event",
            GameMode::ConvoyAirship => "convoy-airship",
            GameMode::TwoTeamsBattleRoyale => "two-teams-battle-royale",
            GameMode::PinataEvent => "pinata-event",
            GameMode::Respawns => "respawns",
            GameMode::RespawnsSectors => "respawns-sectors",
        }
    }
}

/// What the projectile collided with (from CollisionMath module).
/// Mapped from `COLLISION_TYPES` in ships.xml.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "rkyv", derive(rkyv::Archive, rkyv::Serialize, rkyv::Deserialize))]
pub enum CollisionType {
    NoHit,
    HitWater,
    HitGround,
    HitEntity,
    HitEntityBB,
    HitWave,
}

impl CollisionType {
    pub const fn name(&self) -> &'static str {
        match self {
            CollisionType::NoHit => "NO_HIT",
            CollisionType::HitWater => "HIT_WATER",
            CollisionType::HitGround => "HIT_GROUND",
            CollisionType::HitEntity => "HIT_ENTITY",
            CollisionType::HitEntityBB => "HIT_ENTITY_BB",
            CollisionType::HitWave => "HIT_WAVE",
        }
    }
}

impl CollisionType {
    pub fn from_id(id: i32, constants: &ShipsConstants, version: Version) -> Option<Recognized<Self>> {
        constants.collision_type(id).map(|name| Self::from_name(name, version))
    }

    pub fn from_name(name: &str, _version: Version) -> Recognized<Self> {
        match name {
            "NO_HIT" => Recognized::Known(CollisionType::NoHit),
            "HIT_WATER" => Recognized::Known(CollisionType::HitWater),
            "HIT_GROUND" => Recognized::Known(CollisionType::HitGround),
            "HIT_ENTITY" => Recognized::Known(CollisionType::HitEntity),
            "HIT_ENTITY_BB" => Recognized::Known(CollisionType::HitEntityBB),
            "HIT_WAVE" => Recognized::Known(CollisionType::HitWave),
            other => Recognized::Unknown(other.to_string()),
        }
    }
}

impl fmt::Display for CollisionType {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.name())
    }
}

/// Shell penetration result (from ConstantsShip module).
/// Mapped from `SHELL_HIT_TYPES` in ships.xml.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "rkyv", derive(rkyv::Archive, rkyv::Serialize, rkyv::Deserialize))]
pub enum ShellHitType {
    /// Normal penetration (full damage).
    Normal,
    /// Ricochet (shell bounced off armor).
    Ricochet,
    /// Citadel hit (maximum damage).
    MajorHit,
    /// Shatter (failed to penetrate armor).
    NoPenetration,
    /// Overpenetration (shell passed through without detonating).
    Overpenetration,
    /// No shell hit type (non-shell projectiles).
    None,
    /// Exit point of an overpenetration.
    ExitOverpenetration,
    /// Underwater hit.
    Underwater,
}

impl ShellHitType {
    pub const fn name(&self) -> &'static str {
        match self {
            ShellHitType::Normal => "SHELL_HIT_TYPE_NORMAL",
            ShellHitType::Ricochet => "SHELL_HIT_TYPE_RICOCHET",
            ShellHitType::MajorHit => "SHELL_HIT_TYPE_MAJORHIT",
            ShellHitType::NoPenetration => "SHELL_HIT_TYPE_NOPENETRATION",
            ShellHitType::Overpenetration => "SHELL_HIT_TYPE_OVERPENETRATION",
            ShellHitType::None => "SHELL_HIT_TYPE_NONE",
            ShellHitType::ExitOverpenetration => "SHELL_HIT_TYPE_EXIT_OVERPENETRATION",
            ShellHitType::Underwater => "SHELL_HIT_TYPE_UNDERWATER",
        }
    }
}

impl ShellHitType {
    pub fn from_id(id: i32, constants: &ShipsConstants, version: Version) -> Option<Recognized<Self>> {
        constants.shell_hit_type(id).map(|name| Self::from_name(name, version))
    }

    pub fn from_name(name: &str, _version: Version) -> Recognized<Self> {
        match name {
            "SHELL_HIT_TYPE_NORMAL" => Recognized::Known(ShellHitType::Normal),
            "SHELL_HIT_TYPE_RICOCHET" => Recognized::Known(ShellHitType::Ricochet),
            "SHELL_HIT_TYPE_MAJORHIT" => Recognized::Known(ShellHitType::MajorHit),
            "SHELL_HIT_TYPE_NOPENETRATION" => Recognized::Known(ShellHitType::NoPenetration),
            "SHELL_HIT_TYPE_OVERPENETRATION" => Recognized::Known(ShellHitType::Overpenetration),
            "SHELL_HIT_TYPE_NONE" => Recognized::Known(ShellHitType::None),
            "SHELL_HIT_TYPE_EXIT_OVERPENETRATION" => Recognized::Known(ShellHitType::ExitOverpenetration),
            "SHELL_HIT_TYPE_UNDERWATER" => Recognized::Known(ShellHitType::Underwater),
            other => Recognized::Unknown(other.to_string()),
        }
    }
}

impl fmt::Display for ShellHitType {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.name())
    }
}

/// InteractiveZone entity type.
///
/// From `BattleLogicComponentsConstants.InteractiveZoneTypes`, generated via
/// `idGenerator()` (0-based sequential).
#[derive(Debug, PartialEq, Eq, Hash, Clone, Copy)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "rkyv", derive(rkyv::Archive, rkyv::Serialize, rkyv::Deserialize))]
pub enum InteractiveZoneType {
    NoType,
    ResourceZone,
    ConvoyZone,
    RepairZone,
    FelZone,
    WeatherZone,
    DropZone,
    ConsumableZone,
    ColoredByRelation,
    ControlPoint,
    RescueZone,
    OrbitalStrikeZone,
}

impl InteractiveZoneType {
    pub const fn name(&self) -> &'static str {
        match self {
            Self::NoType => "noType",
            Self::ResourceZone => "resourceZone",
            Self::ConvoyZone => "convoyZone",
            Self::RepairZone => "repairZone",
            Self::FelZone => "felZone",
            Self::WeatherZone => "weatherZone",
            Self::DropZone => "dropZone",
            Self::ConsumableZone => "consumableZone",
            Self::ColoredByRelation => "coloredByRelation",
            Self::ControlPoint => "controlPoint",
            Self::RescueZone => "rescue_zone",
            Self::OrbitalStrikeZone => "orbital_strike_zone",
        }
    }
}

impl InteractiveZoneType {
    pub fn from_id(id: i32, constants: &BattleConstants, version: Version) -> Option<Recognized<Self>> {
        constants.interactive_zone_type(id).map(|name| Self::from_name(name, version))
    }

    pub fn from_name(name: &str, _version: Version) -> Recognized<Self> {
        match name {
            "noType" => Recognized::Known(Self::NoType),
            "resourceZone" => Recognized::Known(Self::ResourceZone),
            "convoyZone" => Recognized::Known(Self::ConvoyZone),
            "repairZone" => Recognized::Known(Self::RepairZone),
            "felZone" => Recognized::Known(Self::FelZone),
            "weatherZone" => Recognized::Known(Self::WeatherZone),
            "dropZone" => Recognized::Known(Self::DropZone),
            "consumableZone" => Recognized::Known(Self::ConsumableZone),
            "coloredByRelation" => Recognized::Known(Self::ColoredByRelation),
            "controlPoint" => Recognized::Known(Self::ControlPoint),
            "rescue_zone" => Recognized::Known(Self::RescueZone),
            "orbital_strike_zone" => Recognized::Known(Self::OrbitalStrikeZone),
            other => Recognized::Unknown(other.to_string()),
        }
    }
}

impl fmt::Display for InteractiveZoneType {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.name())
    }
}

/// Control point sub-type within an InteractiveZone.
///
/// From `CapturePointConstants.CONTROL_POINT_TYPE` (in `ma7c29490.pyc`),
/// generated via `idGenerator(start=1)`.
#[derive(Debug, PartialEq, Eq, Hash, Clone, Copy)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "rkyv", derive(rkyv::Archive, rkyv::Serialize, rkyv::Deserialize))]
pub enum ControlPointType {
    Control,
    Base,
    MegaBase,
    BuildingCp,
    BaseWithPoints,
    EpicenterCp,
}

impl ControlPointType {
    pub const fn name(&self) -> &'static str {
        match self {
            Self::Control => "Control",
            Self::Base => "Base",
            Self::MegaBase => "MegaBase",
            Self::BuildingCp => "BuildingCP",
            Self::BaseWithPoints => "BaseWithPoints",
            Self::EpicenterCp => "EpicenterCP",
        }
    }
}

impl ControlPointType {
    pub fn from_id(id: i32, constants: &BattleConstants, version: Version) -> Option<Recognized<Self>> {
        constants.control_point_type(id).map(|name| Self::from_name(name, version))
    }

    pub fn from_name(name: &str, _version: Version) -> Recognized<Self> {
        match name {
            "Control" => Recognized::Known(Self::Control),
            "Base" => Recognized::Known(Self::Base),
            "MegaBase" => Recognized::Known(Self::MegaBase),
            "BuildingCP" => Recognized::Known(Self::BuildingCp),
            "BaseWithPoints" => Recognized::Known(Self::BaseWithPoints),
            "EpicenterCP" => Recognized::Known(Self::EpicenterCp),
            other => Recognized::Unknown(other.to_string()),
        }
    }
}

impl fmt::Display for ControlPointType {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.name())
    }
}

// =============================================================================
// Damage Stat Types (from mc15a2792.pyc, game version 15.1)
// =============================================================================

/// Weapon/damage source for damage stat tracking.
///
/// These correspond to the `enum_weapon = idGenerator(0)` constants from the game's
/// internal `mc15a2792` module. Each value represents a specific combination of weapon
/// system and ammo type. Sent as the first element of the `(weapon, category)` key in
/// the `receiveDamageStat` pickle dict on the Avatar entity.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub enum DamageStatWeapon {
    Default,
    MainAp,
    MainHe,
    AtbaAp,
    AtbaHe,
    MainAiAp,
    MainAiHe,
    Torpedo,
    Antiair,
    Scout,
    BomberAp,
    BomberHe,
    TBomber,
    Fighter,
    SFighter,
    Turret,
    Spot,
    Burn,
    Ram,
    Terrain,
    Flood,
    Mirror,
    Radar,
    Xray,
    ConsSpot,
    SeaMine,
    Fel,
    DepthCharge,
    RocketHe,
    AaNear,
    AaMedium,
    AaFar,
    MainCs,
    AtbaCs,
    Portal,
    TorpedoAcc,
    TorpedoMag,
    Ping,
    PingSlow,
    PingFast,
    TorpedoAccOff,
    RocketAp,
    SkipHe,
    SkipAp,
    Acid,
    SectorWave,
    Match,
    Timer,
    ChargeLaser,
    PulseLaser,
    AxisLaser,
    BomberApAsup,
    BomberHeAsup,
    TBomberAsup,
    RocketHeAsup,
    RocketApAsup,
    SkipHeAsup,
    SkipApAsup,
    DepthChargeAsup,
    TorpedoDeep,
    TorpedoAlter,
    AirSupport,
    BomberApAlter,
    BomberHeAlter,
    TBomberAlter,
    RocketHeAlter,
    RocketApAlter,
    SkipHeAlter,
    SkipApAlter,
    DepthChargeAlter,
    Recon,
    BomberApTc,
    BomberHeTc,
    TBomberTc,
    RocketHeTc,
    RocketApTc,
    SkipHeTc,
    SkipApTc,
    DepthChargeTc,
    PhaserLaser,
    Event1,
    Event2,
    TorpedoPhoton,
    Missile,
    AntiMissile,
}

impl DamageStatWeapon {
    pub const fn name(&self) -> &'static str {
        match self {
            Self::Default => "DEFAULT",
            Self::MainAp => "MAIN_AP",
            Self::MainHe => "MAIN_HE",
            Self::AtbaAp => "ATBA_AP",
            Self::AtbaHe => "ATBA_HE",
            Self::MainAiAp => "MAIN_AI_AP",
            Self::MainAiHe => "MAIN_AI_HE",
            Self::Torpedo => "TORPEDO",
            Self::Antiair => "ANTIAIR",
            Self::Scout => "SCOUT",
            Self::BomberAp => "BOMBER_AP",
            Self::BomberHe => "BOMBER_HE",
            Self::TBomber => "TBOMBER",
            Self::Fighter => "FIGHTER",
            Self::SFighter => "SFIGHTER",
            Self::Turret => "TURRET",
            Self::Spot => "SPOT",
            Self::Burn => "BURN",
            Self::Ram => "RAM",
            Self::Terrain => "TERRAIN",
            Self::Flood => "FLOOD",
            Self::Mirror => "MIRROR",
            Self::Radar => "RADAR",
            Self::Xray => "XRAY",
            Self::ConsSpot => "CONS_SPOT",
            Self::SeaMine => "SEA_MINE",
            Self::Fel => "FEL",
            Self::DepthCharge => "DBOMB",
            Self::RocketHe => "ROCKET_HE",
            Self::AaNear => "AA_NEAR",
            Self::AaMedium => "AA_MEDIUM",
            Self::AaFar => "AA_FAR",
            Self::MainCs => "MAIN_CS",
            Self::AtbaCs => "ATBA_CS",
            Self::Portal => "PORTAL",
            Self::TorpedoAcc => "TORPEDO_ACC",
            Self::TorpedoMag => "TORPEDO_MAG",
            Self::Ping => "PING",
            Self::PingSlow => "PING_SLOW",
            Self::PingFast => "PING_FAST",
            Self::TorpedoAccOff => "TORPEDO_ACC_OFF",
            Self::RocketAp => "ROCKET_AP",
            Self::SkipHe => "SKIP_HE",
            Self::SkipAp => "SKIP_AP",
            Self::Acid => "ACID",
            Self::SectorWave => "SECTOR_WAVE",
            Self::Match => "MATCH",
            Self::Timer => "TIMER",
            Self::ChargeLaser => "CHARGE_LASER",
            Self::PulseLaser => "PULSE_LASER",
            Self::AxisLaser => "AXIS_LASER",
            Self::BomberApAsup => "BOMBER_AP_ASUP",
            Self::BomberHeAsup => "BOMBER_HE_ASUP",
            Self::TBomberAsup => "TBOMBER_ASUP",
            Self::RocketHeAsup => "ROCKET_HE_ASUP",
            Self::RocketApAsup => "ROCKET_AP_ASUP",
            Self::SkipHeAsup => "SKIP_HE_ASUP",
            Self::SkipApAsup => "SKIP_AP_ASUP",
            Self::DepthChargeAsup => "DBOMB_ASUP",
            Self::TorpedoDeep => "TORPEDO_DEEP",
            Self::TorpedoAlter => "TORPEDO_ALTER",
            Self::AirSupport => "AIR_SUPPORT",
            Self::BomberApAlter => "BOMBER_AP_ALTER",
            Self::BomberHeAlter => "BOMBER_HE_ALTER",
            Self::TBomberAlter => "TBOMBER_ALTER",
            Self::RocketHeAlter => "ROCKET_HE_ALTER",
            Self::RocketApAlter => "ROCKET_AP_ALTER",
            Self::SkipHeAlter => "SKIP_HE_ALTER",
            Self::SkipApAlter => "SKIP_AP_ALTER",
            Self::DepthChargeAlter => "DBOMB_ALTER",
            Self::Recon => "RECON",
            Self::BomberApTc => "BOMBER_AP_TC",
            Self::BomberHeTc => "BOMBER_HE_TC",
            Self::TBomberTc => "TBOMBER_TC",
            Self::RocketHeTc => "ROCKET_HE_TC",
            Self::RocketApTc => "ROCKET_AP_TC",
            Self::SkipHeTc => "SKIP_HE_TC",
            Self::SkipApTc => "SKIP_AP_TC",
            Self::DepthChargeTc => "DBOMB_TC",
            Self::PhaserLaser => "PHASER_LASER",
            Self::Event1 => "EVENT_1",
            Self::Event2 => "EVENT_2",
            Self::TorpedoPhoton => "TORPEDO_PHOTON",
            Self::Missile => "MISSILE",
            Self::AntiMissile => "ANTI_MISSILE",
        }
    }
}

impl DamageStatWeapon {
    pub fn from_id(id: i32, constants: &BattleConstants, version: Version) -> Option<Recognized<Self>> {
        constants.damage_stat_weapon(id).map(|name| Self::from_name(name, version))
    }

    pub fn from_name(name: &str, _version: Version) -> Recognized<Self> {
        match name {
            "DEFAULT" => Recognized::Known(Self::Default),
            "MAIN_AP" => Recognized::Known(Self::MainAp),
            "MAIN_HE" => Recognized::Known(Self::MainHe),
            "ATBA_AP" => Recognized::Known(Self::AtbaAp),
            "ATBA_HE" => Recognized::Known(Self::AtbaHe),
            "MAIN_AI_AP" => Recognized::Known(Self::MainAiAp),
            "MAIN_AI_HE" => Recognized::Known(Self::MainAiHe),
            "TORPEDO" => Recognized::Known(Self::Torpedo),
            "ANTIAIR" => Recognized::Known(Self::Antiair),
            "SCOUT" => Recognized::Known(Self::Scout),
            "BOMBER_AP" => Recognized::Known(Self::BomberAp),
            "BOMBER_HE" => Recognized::Known(Self::BomberHe),
            "TBOMBER" => Recognized::Known(Self::TBomber),
            "FIGHTER" => Recognized::Known(Self::Fighter),
            "SFIGHTER" => Recognized::Known(Self::SFighter),
            "TURRET" => Recognized::Known(Self::Turret),
            "SPOT" => Recognized::Known(Self::Spot),
            "BURN" => Recognized::Known(Self::Burn),
            "RAM" => Recognized::Known(Self::Ram),
            "TERRAIN" => Recognized::Known(Self::Terrain),
            "FLOOD" => Recognized::Known(Self::Flood),
            "MIRROR" => Recognized::Known(Self::Mirror),
            "RADAR" => Recognized::Known(Self::Radar),
            "XRAY" => Recognized::Known(Self::Xray),
            "CONS_SPOT" => Recognized::Known(Self::ConsSpot),
            "SEA_MINE" => Recognized::Known(Self::SeaMine),
            "FEL" => Recognized::Known(Self::Fel),
            "DBOMB" => Recognized::Known(Self::DepthCharge),
            "ROCKET_HE" => Recognized::Known(Self::RocketHe),
            "AA_NEAR" => Recognized::Known(Self::AaNear),
            "AA_MEDIUM" => Recognized::Known(Self::AaMedium),
            "AA_FAR" => Recognized::Known(Self::AaFar),
            "MAIN_CS" => Recognized::Known(Self::MainCs),
            "ATBA_CS" => Recognized::Known(Self::AtbaCs),
            "PORTAL" => Recognized::Known(Self::Portal),
            "TORPEDO_ACC" => Recognized::Known(Self::TorpedoAcc),
            "TORPEDO_MAG" => Recognized::Known(Self::TorpedoMag),
            "PING" => Recognized::Known(Self::Ping),
            "PING_SLOW" => Recognized::Known(Self::PingSlow),
            "PING_FAST" => Recognized::Known(Self::PingFast),
            "TORPEDO_ACC_OFF" => Recognized::Known(Self::TorpedoAccOff),
            "ROCKET_AP" => Recognized::Known(Self::RocketAp),
            "SKIP_HE" => Recognized::Known(Self::SkipHe),
            "SKIP_AP" => Recognized::Known(Self::SkipAp),
            "ACID" => Recognized::Known(Self::Acid),
            "SECTOR_WAVE" => Recognized::Known(Self::SectorWave),
            "MATCH" => Recognized::Known(Self::Match),
            "TIMER" => Recognized::Known(Self::Timer),
            "CHARGE_LASER" => Recognized::Known(Self::ChargeLaser),
            "PULSE_LASER" => Recognized::Known(Self::PulseLaser),
            "AXIS_LASER" => Recognized::Known(Self::AxisLaser),
            "BOMBER_AP_ASUP" => Recognized::Known(Self::BomberApAsup),
            "BOMBER_HE_ASUP" => Recognized::Known(Self::BomberHeAsup),
            "TBOMBER_ASUP" => Recognized::Known(Self::TBomberAsup),
            "ROCKET_HE_ASUP" => Recognized::Known(Self::RocketHeAsup),
            "ROCKET_AP_ASUP" => Recognized::Known(Self::RocketApAsup),
            "SKIP_HE_ASUP" => Recognized::Known(Self::SkipHeAsup),
            "SKIP_AP_ASUP" => Recognized::Known(Self::SkipApAsup),
            "DBOMB_ASUP" => Recognized::Known(Self::DepthChargeAsup),
            "TORPEDO_DEEP" => Recognized::Known(Self::TorpedoDeep),
            "TORPEDO_ALTER" => Recognized::Known(Self::TorpedoAlter),
            "AIR_SUPPORT" => Recognized::Known(Self::AirSupport),
            "BOMBER_AP_ALTER" => Recognized::Known(Self::BomberApAlter),
            "BOMBER_HE_ALTER" => Recognized::Known(Self::BomberHeAlter),
            "TBOMBER_ALTER" => Recognized::Known(Self::TBomberAlter),
            "ROCKET_HE_ALTER" => Recognized::Known(Self::RocketHeAlter),
            "ROCKET_AP_ALTER" => Recognized::Known(Self::RocketApAlter),
            "SKIP_HE_ALTER" => Recognized::Known(Self::SkipHeAlter),
            "SKIP_AP_ALTER" => Recognized::Known(Self::SkipApAlter),
            "DBOMB_ALTER" => Recognized::Known(Self::DepthChargeAlter),
            "RECON" => Recognized::Known(Self::Recon),
            "BOMBER_AP_TC" => Recognized::Known(Self::BomberApTc),
            "BOMBER_HE_TC" => Recognized::Known(Self::BomberHeTc),
            "TBOMBER_TC" => Recognized::Known(Self::TBomberTc),
            "ROCKET_HE_TC" => Recognized::Known(Self::RocketHeTc),
            "ROCKET_AP_TC" => Recognized::Known(Self::RocketApTc),
            "SKIP_HE_TC" => Recognized::Known(Self::SkipHeTc),
            "SKIP_AP_TC" => Recognized::Known(Self::SkipApTc),
            "DBOMB_TC" => Recognized::Known(Self::DepthChargeTc),
            "PHASER_LASER" => Recognized::Known(Self::PhaserLaser),
            "EVENT_1" => Recognized::Known(Self::Event1),
            "EVENT_2" => Recognized::Known(Self::Event2),
            "TORPEDO_PHOTON" => Recognized::Known(Self::TorpedoPhoton),
            "MISSILE" => Recognized::Known(Self::Missile),
            "ANTI_MISSILE" => Recognized::Known(Self::AntiMissile),
            other => Recognized::Unknown(other.to_string()),
        }
    }
}

impl fmt::Display for DamageStatWeapon {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.name())
    }
}

/// Category of damage stat tracking.
///
/// These correspond to the `DamageStatsType` constants from the game's internal modules
/// (mc15a2792.pyc, Avatar.pyc). Sent as the second element of the `(weapon, category)`
/// key in the `receiveDamageStat` pickle dict on the Avatar entity.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub enum DamageStatCategory {
    /// Damage dealt to enemies (sub_type=0).
    Enemy,
    /// Damage dealt to allied ships (sub_type=1).
    Ally,
    /// Spotting damage — damage dealt by teammates to targets you spotted (sub_type=2).
    Spot,
    /// Potential damage / "agro" — incoming fire aimed at you (sub_type=3).
    Agro,
}

impl DamageStatCategory {
    pub const fn name(&self) -> &'static str {
        match self {
            Self::Enemy => "ENEMY",
            Self::Ally => "ALLY",
            Self::Spot => "SPOT",
            Self::Agro => "AGRO",
        }
    }
}

impl DamageStatCategory {
    pub fn from_id(id: i32, constants: &BattleConstants, version: Version) -> Option<Recognized<Self>> {
        constants.damage_stat_category(id).map(|name| Self::from_name(name, version))
    }

    pub fn from_name(name: &str, _version: Version) -> Recognized<Self> {
        match name {
            "ENEMY" => Recognized::Known(Self::Enemy),
            "ALLY" => Recognized::Known(Self::Ally),
            "SPOT" => Recognized::Known(Self::Spot),
            "AGRO" => Recognized::Known(Self::Agro),
            other => Recognized::Unknown(other.to_string()),
        }
    }
}

impl fmt::Display for DamageStatCategory {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.name())
    }
}

/// Payload-free counterpart of `HitEligibility`, used as the exclusion-tally
/// key. Separate from `HitEligibility` because the tally counts reasons, not
/// instances, and `BurnNodeIndex`/`ShellHitType` payloads would fragment it.
///
/// Only refusals appear here, and only over the population the eligibility
/// model was asked about: our own main-battery HE hits on a ship. A shell that
/// was never that population's member is a [`NarrowingReason`] instead, and a
/// hit the model was never asked to judge, such as one landing on a ship that
/// was already dead, is counted apart from both.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum ExclusionReason {
    SectionAlreadyBurning,
    MergedSectionVictimBuildUnknown,
    DamageControlActive,
    DamageControlUnknown,
    ObservationGap,
    ConsumableModelUnreliable,
    VictimFateUnknown,
    HitTypeDoesNotRoll,
    NoSectionGeometry,
    ImpactUnplaceableOnVictim,
    VictimPoseUnknown,
    AmbiguousWithAnotherHit,
}

/// Why a `SetFire` ribbon of ours could not be credited to one of our shells.
///
/// The ribbon says a fire was ours and carries neither a victim nor a weapon,
/// so every reason here is stated in terms of what our own shells were doing in
/// the attribution window around it. The most specific reason wins: a ribbon
/// with an eligible hit beside it is described by what happened to that hit, not
/// by the AP shell that also landed nearby.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum UnattributedFireReason {
    /// An eligible hit of ours sat in the window with no burn transition on its
    /// victim to match the ribbon against, so nothing ties the two together.
    /// The ordinary cause is a victim outside the recording client's AOI, whose
    /// `burningFlags` we never see change.
    BurnStateNotObserved,
    /// Every eligible hit in the window, or every burn transition they could
    /// have matched, was already spent on an earlier ribbon. Two fires close
    /// together with one candidate between them land here.
    AlreadyCreditedToAnEarlierFire,
    /// The only hits of ours in the window were dropped because one of our own
    /// secondaries landed in the same section at the same time, so the fire
    /// cannot be told from a secondary's.
    ContestedByOurSecondary,
    /// The only hits of ours in the window were dropped because another hit of
    /// ours whose fire roll could not be ruled out landed in the same section:
    /// a ricochet, an overpenetration, a hit type this build cannot name, or a
    /// hit on the merged fire node of a victim whose build never resolved.
    ContestedByAnotherHitOfOurs,
    /// Every hit of ours in the window was refused by the eligibility model.
    /// This is the one reason that says the model may be wrong rather than that
    /// data is missing: it believed a fire was impossible at a moment the server
    /// lit one.
    EveryNearbyHitExcluded,
    /// Hits of ours landed in the window but none of them could have started a
    /// fire: AP or SAP shells, our own secondaries, shells that struck terrain
    /// or water, and shells that landed on a ship that was already dead. Those
    /// last two groups are merged with the shell-type filter because they share
    /// the only property this bucket asserts, that no fire could have come from
    /// them.
    NoNearbyHitCouldStartAFire,
    /// No shell of ours landed at all in the window. The fire was set by another
    /// player, or by a weapon of ours the model does not track.
    NoHitInWindow,
}

/// Why one of our hits was never a main-battery HE hit on a ship, and so was
/// never a question the eligibility model could answer.
///
/// Apart from [`ExclusionReason`] because these are filters on the shell and on
/// what it struck, not outcomes the model reached. Every HE shell that lands on
/// a ship can start a fire; listing "this was an AP shell" beside "the section
/// was already burning" reads as though some of them could not, which is why
/// this partition is an accounting aid the corpus checks rather than something
/// the breakdown renders.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum NarrowingReason {
    /// AP, SAP, or a main-battery shell whose folded burn chance is not
    /// positive.
    ShellCannotBurn,
    /// A secondary shell. Secondary fire arrives on the same packet path as the
    /// main battery and is separable only by the equipped battery's `ammoList`.
    NotMainBattery,
    /// The shell's collision type says it struck terrain, water, a wave or
    /// nothing at all, so it landed on no ship whatever the nearest-ship
    /// heuristic keyed it to.
    ImpactNotOnAShip,
}

/// A player's Personal Rating band, as wows-numbers.com names them.
///
/// A band is a range on a PR number rather than a stored value, which is what
/// lets a consumer classify a rating it recorded before the band existed. The
/// PR formula and the expected-values data it needs live in
/// `wows-replay-insights`; only the boundaries are here, because the replay
/// search compiles a band filter into a range over an already-indexed PR column
/// and must not pull the replay-parsing stack in to do it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "rkyv", derive(rkyv::Archive, rkyv::Serialize, rkyv::Deserialize))]
pub enum PersonalRatingCategory {
    Bad,
    BelowAverage,
    Average,
    Good,
    VeryGood,
    Great,
    Unicum,
    SuperUnicum,
}

impl PersonalRatingCategory {
    /// Every band, weakest first. The order is the `Ord` order, which
    /// `ceiling` and `from_pr` both read.
    pub const ALL: [PersonalRatingCategory; 8] = [
        Self::Bad,
        Self::BelowAverage,
        Self::Average,
        Self::Good,
        Self::VeryGood,
        Self::Great,
        Self::Unicum,
        Self::SuperUnicum,
    ];

    /// The inclusive PR floor of this band. `None` for `Bad`: nothing sits
    /// below it, so it has no bound rather than a bound of zero.
    ///
    /// The one statement of these constants. `ceiling` and `from_pr` both
    /// derive from it, so a band cannot be moved in one place and left behind
    /// in another.
    pub fn floor(self) -> Option<f64> {
        match self {
            Self::Bad => None,
            Self::BelowAverage => Some(750.0),
            Self::Average => Some(1100.0),
            Self::Good => Some(1350.0),
            Self::VeryGood => Some(1550.0),
            Self::Great => Some(1750.0),
            Self::Unicum => Some(2100.0),
            Self::SuperUnicum => Some(2450.0),
        }
    }

    /// The exclusive PR ceiling of this band: the next band's floor. `None`
    /// for `SuperUnicum`, which is unbounded above.
    pub fn ceiling(self) -> Option<f64> {
        Self::ALL.into_iter().find(|band| *band > self).and_then(Self::floor)
    }

    /// The band a PR value falls in.
    pub fn from_pr(pr: f64) -> Self {
        Self::ALL.into_iter().rev().find(|band| band.floor().is_none_or(|floor| pr >= floor)).unwrap_or(Self::Bad)
    }

    /// The display name for this band.
    pub fn name(&self) -> &'static str {
        match self {
            Self::Bad => "Bad",
            Self::BelowAverage => "Below Average",
            Self::Average => "Average",
            Self::Good => "Good",
            Self::VeryGood => "Very Good",
            Self::Great => "Great",
            Self::Unicum => "Unicum",
            Self::SuperUnicum => "Super Unicum",
        }
    }

    /// The lowercase token this band is written as in the replay search
    /// grammar.
    pub fn as_token(self) -> &'static str {
        match self {
            Self::Bad => "bad",
            Self::BelowAverage => "below-average",
            Self::Average => "average",
            Self::Good => "good",
            Self::VeryGood => "very-good",
            Self::Great => "great",
            Self::Unicum => "unicum",
            Self::SuperUnicum => "super-unicum",
        }
    }

    /// The band `s` names, case-insensitively. The hyphenated `as_token`
    /// spelling and the run-together one are both accepted, so a user who
    /// types `superunicum` reaches the same band the dropdown offers as
    /// `super-unicum`.
    pub fn from_token(s: &str) -> Option<Self> {
        let lower = s.to_ascii_lowercase();
        Self::ALL.into_iter().find(|band| {
            let token = band.as_token();
            lower == token || lower == token.replace('-', "")
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalized_pos_lerp_midpoint() {
        let a = NormalizedPos::new(0.0, 0.0);
        let b = NormalizedPos::new(1.0, 2.0);
        let m = a.lerp(b, 0.5);
        let NormalizedPos(v) = m;
        assert!((v.x - 0.5).abs() < 1e-6);
        assert!((v.y - 1.0).abs() < 1e-6);
    }

    #[test]
    fn gun_bits_expand_to_set_indices() {
        let bits = GunBits::from(0b1011u32);
        let ids: Vec<u32> = bits.gun_ids().map(GunId::raw).collect();
        assert_eq!(ids, vec![0, 1, 3]);
    }

    #[test]
    fn gun_bits_empty_expands_to_nothing() {
        assert_eq!(GunBits::from(0u32).gun_ids().count(), 0);
    }

    #[test]
    fn gun_bits_high_bit_set() {
        let ids: Vec<u32> = GunBits::from(1u32 << 31).gun_ids().map(GunId::raw).collect();
        assert_eq!(ids, vec![31]);
    }

    #[test]
    fn ribbon_from_id_modern_table() {
        assert_eq!(Ribbon::from_id(54), Ribbon::Assist);
        assert_eq!(Ribbon::from_id(15), Ribbon::Penetration);
        assert_eq!(Ribbon::from_id(0), Ribbon::MainCaliber);
    }

    /// Bit values come from the client's `VisionFlags` bit-enum, which assigns
    /// them by declaration order. Pin them so a reordering of `VisionFlag` or
    /// `ALL` cannot silently remap every flag.
    #[test]
    fn vision_flag_bits_match_the_client() {
        assert_eq!(VisionFlag::Ship.bit(), 1);
        assert_eq!(VisionFlag::MainPlane.bit(), 2);
        assert_eq!(VisionFlag::CommonXRay.bit(), 4);
        assert_eq!(VisionFlag::RlsPersonal.bit(), 8);
        assert_eq!(VisionFlag::Rls.bit(), 16);
        assert_eq!(VisionFlag::Sonar.bit(), 32);
        assert_eq!(VisionFlag::Smoke.bit(), 64);
        assert_eq!(VisionFlag::Pinger.bit(), 128);
        assert_eq!(VisionFlag::MiscPlane.bit(), 256);
        assert_eq!(VisionFlag::SubmarineLocator.bit(), 512);
        assert_eq!(VisionFlag::Recon.bit(), 1024);
        assert_eq!(VisionFlag::AntiMissile.bit(), 2048);
        // ALL must stay in ascending bit order for `flags()` to iterate in it.
        let mut expected = 1;
        for flag in VisionFlag::ALL {
            assert_eq!(flag.bit(), expected);
            expected <<= 1;
        }
    }

    #[test]
    fn visibility_flags_decompose() {
        // Observed on the wire: BY_SHIP | IN_SMOKE, a ship firing from smoke.
        let flags = VisibilityFlags::new(65);
        assert!(flags.is_detected());
        assert_eq!(flags.flags().collect::<Vec<_>>(), vec![VisionFlag::Ship, VisionFlag::Smoke]);
        assert_eq!(flags.to_string(), "BY_SHIP|IN_SMOKE");
        assert_eq!(flags.unknown_bits(), 0);

        assert_eq!(VisibilityFlags::default().to_string(), "INVISIBLE");
        assert!(!VisibilityFlags::default().is_detected());

        // BY_RECON | BY_MISC_PLANE | BY_SHIP, observed on a 15.1 replay.
        let planes = VisibilityFlags::new(1281);
        assert!(planes.by_any_plane());
        assert!(!planes.by_xray());

        // Spotter-only sources do not reveal the ship to the spotter's team.
        assert!(!VisibilityFlags::new(VisionFlag::RlsPersonal.bit()).visible_for_team());
        assert!(!VisibilityFlags::new(VisionFlag::Pinger.bit()).visible_for_team());
        assert!(VisibilityFlags::new(VisionFlag::Rls.bit()).visible_for_team());

        // A bit this table does not know is surfaced, not dropped.
        let future = VisibilityFlags::new(1 | 1 << 20);
        assert_eq!(future.unknown_bits(), 1 << 20);
        assert_eq!(future.to_string(), "BY_SHIP|UNKNOWN(0x100000)");
    }

    #[test]
    fn every_documented_game_mode_id_round_trips() {
        for mode in GameMode::ALL {
            let back = GameMode::from_id(mode.id());
            assert_eq!(back.known().copied(), Some(mode), "{mode:?} did not round trip");
        }
    }

    #[test]
    fn the_id_table_matches_the_games_own_values() {
        // Spot-checked against GAME_MODE in the deobfuscated shared_constants.
        // The gaps at 3..=6 are the game's, not an omission here.
        assert_eq!(GameMode::Invalid.id(), -1);
        assert_eq!(GameMode::Standart.id(), 1);
        assert_eq!(GameMode::Domination.id(), 7);
        assert_eq!(GameMode::ArmsRace.id(), 15);
        assert_eq!(GameMode::RespawnsSectors.id(), 31);
        for gap in 3..=6 {
            assert!(GameMode::from_id(gap).known().is_none(), "{gap} is a gap in the game's table");
        }
    }

    #[test]
    fn an_unknown_id_degrades_rather_than_failing() {
        // A future build can add a mode. It must survive as its raw id, not
        // collapse onto a neighbouring variant.
        match GameMode::from_id(9_001) {
            Recognized::Unknown(raw) => assert_eq!(raw, 9_001),
            other => panic!("got {other:?}"),
        }
    }

    #[test]
    fn tokens_are_unique_and_kebab_case() {
        let mut seen = std::collections::BTreeSet::new();
        for mode in GameMode::ALL {
            let token = mode.as_token();
            assert!(seen.insert(token), "{token} is offered by two modes");
            assert!(
                token.bytes().all(|b| b.is_ascii_lowercase() || b == b'-' || b.is_ascii_digit()),
                "{token} is not kebab-case"
            );
        }
    }

    #[test]
    fn all_carries_exactly_the_games_id_table() {
        // Transcribed from GAME_MODE in the deobfuscated shared_constants. The
        // gaps at 3..=6 are the game's. A changed or swapped id fails here.
        let expected: [i32; 29] = [
            -1, 0, 1, 2, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16, 17, 18, 19, 20, 21, 22, 23, 24, 25, 26, 27, 28, 29, 30,
            31,
        ];
        let actual: Vec<i32> = GameMode::ALL.iter().map(|m| m.id()).collect();
        assert_eq!(actual, expected.to_vec());
    }

    #[test]
    fn is_offerable_excludes_exactly_the_modes_whose_id_does_not_fit_the_wire_type() {
        // `ReplayMeta.gameMode` is `u32` on the wire, so an id a `u32` cannot
        // hold can never appear in an indexed row, which is what makes the
        // predicate exactly `u32::try_from(id()).is_ok()` rather than a
        // hand-picked exclusion list.
        for mode in GameMode::ALL {
            assert_eq!(mode.is_offerable(), u32::try_from(mode.id()).is_ok(), "{mode:?}");
        }
        let excluded: Vec<GameMode> = GameMode::ALL.into_iter().filter(|m| !m.is_offerable()).collect();
        assert_eq!(excluded, vec![GameMode::Invalid], "the offerable set must narrow by exactly Invalid, no more");
    }

    #[test]
    fn pr_category_boundaries() {
        assert_eq!(PersonalRatingCategory::from_pr(0.0), PersonalRatingCategory::Bad);
        assert_eq!(PersonalRatingCategory::from_pr(749.0), PersonalRatingCategory::Bad);
        assert_eq!(PersonalRatingCategory::from_pr(750.0), PersonalRatingCategory::BelowAverage);
        assert_eq!(PersonalRatingCategory::from_pr(1099.0), PersonalRatingCategory::BelowAverage);
        assert_eq!(PersonalRatingCategory::from_pr(1100.0), PersonalRatingCategory::Average);
        assert_eq!(PersonalRatingCategory::from_pr(1349.0), PersonalRatingCategory::Average);
        assert_eq!(PersonalRatingCategory::from_pr(1350.0), PersonalRatingCategory::Good);
        assert_eq!(PersonalRatingCategory::from_pr(1549.0), PersonalRatingCategory::Good);
        assert_eq!(PersonalRatingCategory::from_pr(1550.0), PersonalRatingCategory::VeryGood);
        assert_eq!(PersonalRatingCategory::from_pr(1749.0), PersonalRatingCategory::VeryGood);
        assert_eq!(PersonalRatingCategory::from_pr(1750.0), PersonalRatingCategory::Great);
        assert_eq!(PersonalRatingCategory::from_pr(2099.0), PersonalRatingCategory::Great);
        assert_eq!(PersonalRatingCategory::from_pr(2100.0), PersonalRatingCategory::Unicum);
        assert_eq!(PersonalRatingCategory::from_pr(2449.0), PersonalRatingCategory::Unicum);
        assert_eq!(PersonalRatingCategory::from_pr(2450.0), PersonalRatingCategory::SuperUnicum);
        assert_eq!(PersonalRatingCategory::from_pr(5000.0), PersonalRatingCategory::SuperUnicum);
    }

    #[test]
    fn pr_category_names() {
        assert_eq!(PersonalRatingCategory::Bad.name(), "Bad");
        assert_eq!(PersonalRatingCategory::BelowAverage.name(), "Below Average");
        assert_eq!(PersonalRatingCategory::Average.name(), "Average");
        assert_eq!(PersonalRatingCategory::Good.name(), "Good");
        assert_eq!(PersonalRatingCategory::VeryGood.name(), "Very Good");
        assert_eq!(PersonalRatingCategory::Great.name(), "Great");
        assert_eq!(PersonalRatingCategory::Unicum.name(), "Unicum");
        assert_eq!(PersonalRatingCategory::SuperUnicum.name(), "Super Unicum");
    }

    #[test]
    fn pr_category_ordering() {
        for pair in PersonalRatingCategory::ALL.windows(2) {
            assert!(pair[0] < pair[1], "{:?} must sort below {:?}", pair[0], pair[1]);
        }
    }

    #[test]
    fn every_band_token_round_trips_and_accepts_the_run_together_spelling() {
        for band in PersonalRatingCategory::ALL {
            assert_eq!(PersonalRatingCategory::from_token(band.as_token()), Some(band));
            assert_eq!(PersonalRatingCategory::from_token(&band.as_token().to_ascii_uppercase()), Some(band));
            assert_eq!(PersonalRatingCategory::from_token(&band.as_token().replace('-', "")), Some(band));
        }
        assert_eq!(PersonalRatingCategory::from_token("superunicum"), Some(PersonalRatingCategory::SuperUnicum));
        assert_eq!(PersonalRatingCategory::from_token("legendary"), None);
    }

    /// Each band's ceiling is the next band's floor, and the chain is closed at
    /// both ends. A band whose ceiling drifted off the next band's floor would
    /// leave a PR value belonging to no band or to two.
    #[test]
    fn the_bands_tile_the_pr_line_with_no_gap_and_no_overlap() {
        assert_eq!(PersonalRatingCategory::Bad.floor(), None, "Bad must have no floor");
        assert_eq!(PersonalRatingCategory::SuperUnicum.ceiling(), None, "SuperUnicum must have no ceiling");
        for pair in PersonalRatingCategory::ALL.windows(2) {
            let (lower, upper) = (pair[0], pair[1]);
            assert_eq!(lower.ceiling(), upper.floor(), "{lower:?} does not meet {upper:?}");
            let boundary = upper.floor().expect("only Bad has no floor");
            assert_eq!(PersonalRatingCategory::from_pr(boundary), upper);
            assert_eq!(PersonalRatingCategory::from_pr(boundary - 1.0), lower);
        }
    }
}

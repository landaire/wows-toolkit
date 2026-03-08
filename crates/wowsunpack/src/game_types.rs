//! Game concept types that describe World of Warships mechanics.
//!
//! These types represent game entities, identifiers, positions, and enumerations
//! that are useful across any tool working with WoWS data -- not just replay parsers.

use std::fmt;

#[cfg(feature = "parsing")]
use crate::data::Version;
#[cfg(feature = "parsing")]
use crate::game_constants::BattleConstants;
#[cfg(feature = "parsing")]
use crate::game_constants::CommonConstants;
#[cfg(feature = "parsing")]
use crate::game_constants::ShipsConstants;
#[cfg(feature = "parsing")]
use crate::recognized::Recognized;

#[cfg(feature = "parsing")]
use crate::game_params::types::Meters;

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

/// A persistent player account identifier (db_id, avatar_id).
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, Hash)]
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

/// World-space position in BigWorld coordinates.
/// X = east/west, Y = up/down (altitude), Z = north/south. Origin at map center.
#[derive(Debug, Clone, Copy, Default)]
#[cfg_attr(feature = "serde", derive(serde::Serialize))]
#[cfg_attr(feature = "rkyv", derive(rkyv::Archive, rkyv::Serialize, rkyv::Deserialize))]
pub struct WorldPos {
    pub x: f32,
    pub y: f32,
    pub z: f32,
}

impl WorldPos {
    pub fn lerp(self, other: WorldPos, t: f32) -> WorldPos {
        self + (other - self) * t
    }
}

impl std::ops::Add for WorldPos {
    type Output = WorldPos;
    fn add(self, rhs: WorldPos) -> WorldPos {
        WorldPos { x: self.x + rhs.x, y: self.y + rhs.y, z: self.z + rhs.z }
    }
}

impl std::ops::Sub for WorldPos {
    type Output = WorldPos;
    fn sub(self, rhs: WorldPos) -> WorldPos {
        WorldPos { x: self.x - rhs.x, y: self.y - rhs.y, z: self.z - rhs.z }
    }
}

impl std::ops::Mul<f32> for WorldPos {
    type Output = WorldPos;
    fn mul(self, rhs: f32) -> WorldPos {
        WorldPos { x: self.x * rhs, y: self.y * rhs, z: self.z * rhs }
    }
}

impl std::ops::Div<f32> for WorldPos {
    type Output = WorldPos;
    fn div(self, rhs: f32) -> WorldPos {
        WorldPos { x: self.x / rhs, y: self.y / rhs, z: self.z / rhs }
    }
}

impl std::iter::Sum for WorldPos {
    fn sum<I: Iterator<Item = Self>>(iter: I) -> Self {
        iter.fold(WorldPos::default(), |a, b| WorldPos { x: a.x + b.x, y: a.y + b.y, z: a.z + b.z })
    }
}

#[cfg(feature = "parsing")]
impl WorldPos {
    /// Horizontal (XZ-plane) distance to another position, returned in meters.
    /// Both positions are in BigWorld coordinates (1 BW = 30m).
    pub fn distance_xz(&self, other: &WorldPos) -> Meters {
        let dx = (self.x - other.x) * 30.0;
        let dz = (self.z - other.z) * 30.0;
        Meters::from((dx * dx + dz * dz).sqrt())
    }
}

/// 2D world-space position (X/Z plane) for entities that lack altitude data,
/// such as minimap plane squadron positions.
#[derive(Debug, Clone, Copy, Default)]
#[cfg_attr(feature = "serde", derive(serde::Serialize))]
#[cfg_attr(feature = "rkyv", derive(rkyv::Archive, rkyv::Serialize, rkyv::Deserialize))]
pub struct WorldPos2D {
    pub x: f32,
    pub z: f32,
}

impl WorldPos2D {
    /// Promote to 3D with `y = 0.0`.
    pub fn to_world_pos(self) -> WorldPos {
        WorldPos { x: self.x, y: 0.0, z: self.z }
    }
}

/// Normalized minimap position from MinimapUpdate packets.
/// Values roughly in [-0.5, 1.5] range (centered around [0,1]).
#[derive(Debug, Clone, Copy)]
#[cfg_attr(feature = "serde", derive(serde::Serialize))]
#[cfg_attr(feature = "rkyv", derive(rkyv::Archive, rkyv::Serialize, rkyv::Deserialize))]
pub struct NormalizedPos {
    pub x: f32,
    pub y: f32,
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
#[derive(Debug, PartialEq, Eq, Hash, Clone, Copy)]
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
            Ribbon::SonarTwoHits => None, // No known results key
            Ribbon::SonarNeutralized => None, // No known results key
            Ribbon::Unknown(_) => None,
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

#[cfg(feature = "parsing")]
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

#[cfg(feature = "parsing")]
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

#[cfg(feature = "parsing")]
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

#[cfg(feature = "parsing")]
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

#[cfg(feature = "parsing")]
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

#[cfg(feature = "parsing")]
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
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
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
}

#[cfg(feature = "parsing")]
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

#[cfg(feature = "parsing")]
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

#[cfg(feature = "parsing")]
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

#[cfg(feature = "parsing")]
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

#[cfg(feature = "parsing")]
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

#[cfg(feature = "parsing")]
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

#[cfg(feature = "parsing")]
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

#[cfg(feature = "parsing")]
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

#[cfg(feature = "parsing")]
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

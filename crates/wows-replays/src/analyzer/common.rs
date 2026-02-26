/// Enumerates voicelines which can be said in the game.
#[derive(Debug, Clone, Copy, Serialize)]
pub enum VoiceLine {
    IntelRequired,
    FairWinds,
    Wilco,
    Negative,
    WellDone,
    Curses,
    UsingRadar,
    UsingHydroSearch,
    DefendTheBase, // TODO: ...except when it's "thank you"?
    SetSmokeScreen,
    FollowMe,
    // TODO: definitely has associated data similar to AttentionToSquare
    MapPointAttention,
    UsingSubmarineLocator,
    /// "Provide anti-aircraft support"
    ProvideAntiAircraft,
    /// If a player is called out in the message, their avatar ID will be here.
    RequestingSupport(Option<u32>),
    /// If a player is called out in the message, their avatar ID will be here.
    Retreat(Option<i32>),

    /// The position is (letter,number) and zero-indexed. e.g. F2 is (5,1)
    AttentionToSquare((u32, u32)),

    /// Field is the avatar ID of the target
    ConcentrateFire(i32),
}

/// Enumerates the ribbons which appear in the top-right
#[derive(Debug, PartialEq, Eq, Hash, Clone, Copy, Serialize)]
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

#[derive(Debug, PartialEq, Eq, Hash, Clone, Copy, Serialize)]
pub enum DeathCause {
    Secondaries,
    Artillery,
    Fire,
    Flooding,
    Torpedo,
    DiveBomber,
    AerialRocket,
    AerialTorpedo,
    Detonation,
    Ramming,
    DepthCharge,
    SkipBombs,
    Unknown(u32),
}

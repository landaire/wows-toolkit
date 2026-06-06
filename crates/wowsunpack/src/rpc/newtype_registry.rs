//! Maps game def semantic type names (from alias.xml) to the domain newtype
//! that represents them in this codebase. Used by the type-coverage audit.

/// A domain newtype that a def semantic name corresponds to.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SemanticNewtype {
    EntityId,
    TeamId,
    AccountId,
    GameParamId,
    PlaneId,
    ShotId,
}

impl SemanticNewtype {
    /// The Rust type name, for audit reporting.
    pub fn rust_type_name(self) -> &'static str {
        match self {
            Self::EntityId => "EntityId",
            Self::TeamId => "TeamId",
            Self::AccountId => "AccountId",
            Self::GameParamId => "GameParamId",
            Self::PlaneId => "PlaneId",
            Self::ShotId => "ShotId",
        }
    }
}

/// The newtype a def semantic name maps to, if one exists today. Grown from
/// audit output as alias spellings are confirmed against real game defs.
pub fn newtype_for(def_name: &str) -> Option<SemanticNewtype> {
    Some(match def_name {
        "ENTITY_ID" => SemanticNewtype::EntityId,
        "TEAM_ID" => SemanticNewtype::TeamId,
        "DB_ID" => SemanticNewtype::AccountId,
        "GAMEPARAMS_ID" => SemanticNewtype::GameParamId,
        "PLANE_ID" => SemanticNewtype::PlaneId,
        "SHOT_ID" => SemanticNewtype::ShotId,
        _ => return None,
    })
}

#[cfg(test)]
mod test {
    use super::*;

    #[test]
    fn known_and_unknown_names() {
        assert_eq!(newtype_for("ENTITY_ID"), Some(SemanticNewtype::EntityId));
        assert_eq!(newtype_for("TEAM_ID"), Some(SemanticNewtype::TeamId));
        assert_eq!(newtype_for("DB_ID"), Some(SemanticNewtype::AccountId));
        assert_eq!(newtype_for("GAMEPARAMS_ID"), Some(SemanticNewtype::GameParamId));
        assert_eq!(newtype_for("PLANE_ID"), Some(SemanticNewtype::PlaneId));
        assert_eq!(newtype_for("SHOT_ID"), Some(SemanticNewtype::ShotId));
        assert_eq!(newtype_for("NOT_A_REAL_ALIAS"), None);
        assert_eq!(SemanticNewtype::EntityId.rust_type_name(), "EntityId");
        assert_eq!(SemanticNewtype::AccountId.rust_type_name(), "AccountId");
        assert_eq!(SemanticNewtype::GameParamId.rust_type_name(), "GameParamId");
        assert_eq!(SemanticNewtype::PlaneId.rust_type_name(), "PlaneId");
        assert_eq!(SemanticNewtype::ShotId.rust_type_name(), "ShotId");
    }
}

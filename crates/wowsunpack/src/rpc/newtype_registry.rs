//! Maps game def semantic type names (from alias.xml) to the domain newtype
//! that represents them in this codebase. Used by the type-coverage audit.

/// A domain newtype that a def semantic name corresponds to.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SemanticNewtype {
    EntityId,
    TeamId,
}

impl SemanticNewtype {
    /// The Rust type name, for audit reporting.
    pub fn rust_type_name(self) -> &'static str {
        match self {
            Self::EntityId => "EntityId",
            Self::TeamId => "TeamId",
        }
    }
}

/// The newtype a def semantic name maps to, if one exists today. Grown from
/// audit output as alias spellings are confirmed against real game defs.
pub fn newtype_for(def_name: &str) -> Option<SemanticNewtype> {
    Some(match def_name {
        "ENTITY_ID" => SemanticNewtype::EntityId,
        "TEAM_ID" => SemanticNewtype::TeamId,
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
        assert_eq!(newtype_for("NOT_A_REAL_ALIAS"), None);
        assert_eq!(SemanticNewtype::EntityId.rust_type_name(), "EntityId");
    }
}

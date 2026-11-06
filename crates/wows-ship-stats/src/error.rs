use wowsunpack::game_params::ttx::modifiers::ModifierError;

#[derive(Debug, thiserror::Error)]
pub enum BuildError {
    #[error("{param} is not a ship")]
    NotAShip { param: String },

    #[error("{ship} has no known ship class")]
    UnknownSpecies { ship: String },

    #[error("failed to apply modifiers to {ship}")]
    Modifiers {
        ship: String,
        #[source]
        source: ModifierError,
    },
}

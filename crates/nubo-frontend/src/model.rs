//! The rule that picks the hardware a game starts on: a Game Boy Color game
//! on a Game Boy Color, any other game on the original Game Boy, unless the
//! player picked one model for every game. Games that use the Super Game
//! Boy's colors and border also run on a plain Game Boy (the cartridge header
//! can't say a game needs one), so they start there too; the player picks
//! the Super Game Boy to see them.

use nubo_core::{CartridgeHeader, Model};
use serde::{Deserialize, Serialize};

/// Which hardware a game starts on. Saved as "auto" or as the model's short
/// name, such as "CGB-E".
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(into = "String", try_from = "String")]
pub enum ModelPreference {
    /// Game Boy Color games start on a Game Boy Color (CGB-E), other games
    /// on the original Game Boy (DMG), even those that also use the Super
    /// Game Boy's colors and border.
    #[default]
    Automatic,
    /// Every game starts on this model.
    Fixed(Model),
}

impl ModelPreference {
    /// The model to start the game with this header on.
    pub fn model_for(self, header: &CartridgeHeader) -> Model {
        match self {
            ModelPreference::Automatic if header.supports_cgb => Model::CgbE,
            ModelPreference::Automatic => Model::Dmg,
            ModelPreference::Fixed(model) => model,
        }
    }
}

impl From<ModelPreference> for String {
    fn from(preference: ModelPreference) -> String {
        match preference {
            ModelPreference::Automatic => String::from("auto"),
            ModelPreference::Fixed(model) => String::from(model.name()),
        }
    }
}

impl TryFrom<String> for ModelPreference {
    type Error = String;

    fn try_from(name: String) -> Result<Self, Self::Error> {
        if name.eq_ignore_ascii_case("auto") {
            Ok(ModelPreference::Automatic)
        } else {
            name.parse().map(ModelPreference::Fixed)
        }
    }
}

#[cfg(test)]
mod tests;

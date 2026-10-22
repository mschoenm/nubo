//! The rule that picks the hardware a game starts on: a Game Boy Color game
//! on a Game Boy Color, a game made for the Super Game Boy on a Super Game
//! Boy (for its colors and border), any other game on the original Game Boy,
//! unless the player picked one model for every game.
//!
//! Apps let the player pick a model as a [`Console`] and then one of its
//! revisions, with [`about`] saying what each one is, and [`made_for`] says
//! which model a game was made for.

use nubo_core::{CartridgeHeader, Model};
use serde::{Deserialize, Serialize};

/// Which hardware a game starts on. Saved as "auto" or as the model's short
/// name, such as "CGB-E".
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(into = "String", try_from = "String")]
pub enum ModelPreference {
    /// Game Boy Color games start on a Game Boy Color (CGB-E), Super Game
    /// Boy games on a Super Game Boy (SGB), other games on the original Game
    /// Boy (DMG). A game made for both the Game Boy Color and the Super Game
    /// Boy starts on the Game Boy Color, where it has the most colors.
    #[default]
    Automatic,
    /// Every game starts on this model.
    Fixed(Model),
}

impl ModelPreference {
    /// The model to start the game with this header on.
    pub fn model_for(self, header: &CartridgeHeader) -> Model {
        match self {
            ModelPreference::Automatic => made_for(header.supports_cgb, header.supports_sgb),
            ModelPreference::Fixed(model) => model,
        }
    }
}

/// The model a game was made for, which Automatic starts it on, from what
/// its cartridge header says: whether it uses the Game Boy Color's and the
/// Super Game Boy's features.
pub fn made_for(supports_cgb: bool, supports_sgb: bool) -> Model {
    if supports_cgb {
        Model::CgbE
    } else if supports_sgb {
        Model::Sgb
    } else {
        Model::Dmg
    }
}

/// A kind of console, which a player picks before its revision: the
/// models grouped as the boxes in a shop were. Apps show these first and the
/// revisions of the picked console next to them, so every model can be
/// picked without a long list.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Console {
    /// The original Game Boy (DMG).
    GameBoy,
    /// The Game Boy Pocket (MGB).
    Pocket,
    /// The Super Game Boy cartridge for the Super Nintendo (SGB).
    SuperGameBoy,
    /// The Game Boy Color (CGB).
    Color,
    /// The Game Boy Advance playing Game Boy games (AGB).
    Advance,
}

impl Console {
    /// Every console, in the order apps list them.
    pub const ALL: [Console; 5] = [
        Console::GameBoy,
        Console::Pocket,
        Console::SuperGameBoy,
        Console::Color,
        Console::Advance,
    ];

    /// The console's name, such as "Game Boy Color".
    pub fn label(self) -> &'static str {
        match self {
            Console::GameBoy => "Game Boy",
            Console::Pocket => "Game Boy Pocket",
            Console::SuperGameBoy => "Super Game Boy",
            Console::Color => "Game Boy Color",
            Console::Advance => "Game Boy Advance",
        }
    }

    /// A shorter name, for a row of buttons: "Pocket", "Color".
    pub fn short_label(self) -> &'static str {
        match self {
            Console::GameBoy => "Game Boy",
            Console::Pocket => "Pocket",
            Console::SuperGameBoy => "Super Game Boy",
            Console::Color => "Color",
            Console::Advance => "Advance",
        }
    }

    /// The console's models, oldest first.
    pub fn revisions(self) -> &'static [Model] {
        match self {
            Console::GameBoy => &[Model::Dmg0, Model::Dmg],
            Console::Pocket => &[Model::Mgb],
            Console::SuperGameBoy => &[Model::Sgb, Model::Sgb2],
            Console::Color => &[
                Model::Cgb0,
                Model::CgbA,
                Model::CgbB,
                Model::CgbC,
                Model::CgbD,
                Model::CgbE,
            ],
            Console::Advance => &[Model::Agb],
        }
    }

    /// The model picking the console starts on: the one most players had.
    pub fn recommended(self) -> Model {
        match self {
            Console::GameBoy => Model::Dmg,
            Console::Pocket => Model::Mgb,
            Console::SuperGameBoy => Model::Sgb,
            Console::Color => Model::CgbE,
            Console::Advance => Model::Agb,
        }
    }

    /// The console `model` belongs to.
    pub fn of(model: Model) -> Console {
        Console::ALL
            .into_iter()
            .find(|console| console.revisions().contains(&model))
            .unwrap_or(Console::GameBoy)
    }
}

/// A sentence about `model` for a player picking it: what it is and how it
/// differs from the others of its console.
pub fn about(model: Model) -> &'static str {
    match model {
        Model::Dmg0 => {
            "The very first Game Boy, sold only in Japan in 1989. Its boot screen differs."
        }
        Model::Dmg => "The original Game Boy, as most people had it (DMG-A, B and C).",
        Model::Mgb => "The smaller Game Boy, with a black and white screen.",
        Model::Sgb => {
            "Plays Game Boy games on a Super Nintendo, with colors and a border. Automatic \
             uses it for games made for it."
        }
        Model::Sgb2 => {
            "The later Japanese Super Game Boy 2. Games can tell it apart from the first one."
        }
        Model::Cgb0 => "The Game Boy Color's first revision, from early Japanese units.",
        Model::CgbA | Model::CgbB | Model::CgbC | Model::CgbD => {
            "A Game Boy Color with a later CPU revision; a few timings differ."
        }
        Model::CgbE => "The Game Boy Color's last revision, which Automatic uses.",
        Model::Agb => "A Game Boy Advance playing Game Boy and Game Boy Color games.",
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

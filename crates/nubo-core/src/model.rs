//! The Game Boy hardware models Nubo can emulate.

use std::fmt;
use std::str::FromStr;

/// A Game Boy hardware model.
///
/// Each model was sold with slightly different chips, and some games and test
/// ROMs notice the difference. The names are the ones printed on the console
/// or its CPU (DMG = "Dot Matrix Game", the original Game Boy; CGB = "Color
/// Game Boy"). The variants are ordered like SameBoy's model numbers, so
/// revision checks can compare them, as in `model <= Model::CgbC`.
///
/// For the Super Game Boys only the differences in the Game Boy hardware are
/// emulated, not their screen borders and color palettes.
///
/// The model is chosen when the emulator is made (`Emulator::new`). Each one
/// leaves the boot ROM in its own state: the CPU registers in
/// `cpu/post_boot.rs`, and the DIV phase, the PPU's place in the frame and
/// the phase of the boot sound in `address_bus/boot_rom.rs`. Work RAM powers
/// on with each model's own pattern (`power_on_work_ram` in `address_bus.rs`).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Model {
    /// The first Game Boy CPU revision, found only in early Japanese units.
    Dmg0,
    /// The original Game Boy with a later CPU revision (A, B or C), which
    /// Nubo treats as one model.
    #[default]
    Dmg,
    /// The Super Game Boy: a Super Nintendo cartridge that plays Game Boy
    /// games on a TV.
    Sgb,
    /// The Game Boy Pocket, a smaller Game Boy.
    Mgb,
    /// The Super Game Boy 2, a revised Super Game Boy sold only in Japan.
    Sgb2,
    /// The first Game Boy Color CPU revision.
    Cgb0,
    /// Game Boy Color CPU revision A.
    CgbA,
    /// Game Boy Color CPU revision B.
    CgbB,
    /// Game Boy Color CPU revision C.
    CgbC,
    /// Game Boy Color CPU revision D.
    CgbD,
    /// Game Boy Color CPU revision E, the last one.
    CgbE,
    /// The Game Boy Advance, playing Game Boy and Game Boy Color games.
    Agb,
}

impl Model {
    /// Every model, in the order a model picker should list them.
    pub const ALL: [Model; 12] = [
        Model::Dmg0,
        Model::Dmg,
        Model::Mgb,
        Model::Sgb,
        Model::Sgb2,
        Model::Cgb0,
        Model::CgbA,
        Model::CgbB,
        Model::CgbC,
        Model::CgbD,
        Model::CgbE,
        Model::Agb,
    ];

    /// Whether this model has Game Boy Color hardware. The Game Boy Advance
    /// does too.
    pub fn is_cgb(self) -> bool {
        self >= Model::Cgb0
    }

    /// Whether this model is a Super Game Boy, which runs Game Boy games on a
    /// Super Nintendo.
    pub fn is_sgb(self) -> bool {
        matches!(self, Model::Sgb | Model::Sgb2)
    }

    /// The short name of the model, such as "CGB-E". Parsing it gives the
    /// model back. Save states store it, so a model's name never changes.
    pub fn name(self) -> &'static str {
        match self {
            Model::Dmg0 => "DMG-0",
            Model::Dmg => "DMG",
            Model::Mgb => "MGB",
            Model::Sgb => "SGB",
            Model::Sgb2 => "SGB2",
            Model::Cgb0 => "CGB-0",
            Model::CgbA => "CGB-A",
            Model::CgbB => "CGB-B",
            Model::CgbC => "CGB-C",
            Model::CgbD => "CGB-D",
            Model::CgbE => "CGB-E",
            Model::Agb => "AGB",
        }
    }

    /// A longer, human readable name, such as "Game Boy Pocket".
    pub fn description(self) -> &'static str {
        match self {
            Model::Dmg0 => "Game Boy (DMG-0, early Japanese units)",
            Model::Dmg => "Game Boy (DMG-A/B/C)",
            Model::Mgb => "Game Boy Pocket",
            Model::Sgb => "Super Game Boy",
            Model::Sgb2 => "Super Game Boy 2",
            Model::Cgb0 => "Game Boy Color (CPU CGB 0)",
            Model::CgbA => "Game Boy Color (CPU CGB A)",
            Model::CgbB => "Game Boy Color (CPU CGB B)",
            Model::CgbC => "Game Boy Color (CPU CGB C)",
            Model::CgbD => "Game Boy Color (CPU CGB D)",
            Model::CgbE => "Game Boy Color (CPU CGB E)",
            Model::Agb => "Game Boy Advance",
        }
    }
}

impl fmt::Display for Model {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.name())
    }
}

/// Parses a model name, ignoring case and punctuation: "cgb-c", "CGBC" and
/// "cgb c" all give [`Model::CgbC`]. A few aliases work too: "CGB" is the
/// CGB-E, "Pocket" the MGB and "GBA" the AGB.
impl FromStr for Model {
    type Err = String;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        let normalized: String = value
            .chars()
            .filter(char::is_ascii_alphanumeric)
            .collect::<String>()
            .to_ascii_uppercase();
        let model = match normalized.as_str() {
            "DMG0" => Model::Dmg0,
            "DMG" | "DMGA" | "DMGB" | "DMGC" => Model::Dmg,
            "MGB" | "POCKET" => Model::Mgb,
            "SGB" => Model::Sgb,
            "SGB2" => Model::Sgb2,
            "CGB0" => Model::Cgb0,
            "CGBA" => Model::CgbA,
            "CGBB" => Model::CgbB,
            "CGBC" => Model::CgbC,
            "CGBD" => Model::CgbD,
            "CGB" | "CGBE" => Model::CgbE,
            "AGB" | "GBA" => Model::Agb,
            _ => return Err(format!("unknown model '{value}'")),
        };
        Ok(model)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn orders_models_like_their_hardware_revisions() {
        assert!(Model::Dmg < Model::Sgb);
        assert!(Model::Sgb < Model::Mgb);
        assert!(Model::CgbC < Model::CgbD);
        assert!(Model::CgbE < Model::Agb);
    }

    #[test]
    fn only_cgb_and_agb_models_are_cgb() {
        assert!(!Model::Sgb2.is_cgb());
        assert!(Model::Cgb0.is_cgb());
        assert!(Model::Agb.is_cgb());
    }

    #[test]
    fn parses_model_names() {
        assert_eq!("cgb-c".parse(), Ok(Model::CgbC));
        assert_eq!("CGB".parse(), Ok(Model::CgbE));
        assert_eq!("dmg".parse(), Ok(Model::Dmg));
        assert_eq!("sgb2".parse(), Ok(Model::Sgb2));
        assert!("nes".parse::<Model>().is_err());
    }

    #[test]
    fn round_trips_names() {
        for model in Model::ALL {
            assert_eq!(model.name().parse(), Ok(model));
        }
    }
}

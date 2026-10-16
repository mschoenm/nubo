//! GameShark and Game Genie cheat codes.
//!
//! Both were cheat devices that sat between the console and the cartridge.
//! A GameShark code makes a byte of RAM read as another value; a Game Genie
//! code patches a byte of the cartridge ROM. Nubo applies both when the
//! game reads memory, without changing what is stored.

use crate::address_bus::AddressBus;
use crate::error::CheatError;
use std::ops::Range;

/// The most cheats that can be active at once.
const CHEAT_LIMIT: usize = 10;

/// A single active cheat: whenever the game reads `address`, it sees
/// `new_value` instead of what is really stored there.
#[derive(Clone, Debug)]
pub(super) struct Cheat {
    /// The address whose reads the cheat changes.
    pub address: u16,
    /// The value those reads see instead.
    pub new_value: u8,
    /// Game Genie codes only apply while the original byte has this value.
    pub compare_value: Option<u8>,
    /// GameShark codes only apply while this RAM bank is selected. `None`
    /// means the cheat applies whatever bank is selected.
    pub bank: Option<u8>,
}

/// An active cheat and the id the frontend chose for it.
#[derive(Clone, Debug)]
struct RegisteredCheat {
    id: String,
    cheat: Cheat,
}

/// The cheats the player has turned on, in the order they were turned on.
/// When several change the same byte, the first one that applies wins, like
/// in SameBoy. A list keeps that order (and the emulator's `Debug` output)
/// the same on every run, which a hash map wouldn't; there are at most
/// `CHEAT_LIMIT` cheats, so searching it is quick.
#[derive(Clone, Debug)]
pub(crate) struct CheatState {
    registered: Vec<RegisteredCheat>,
}

/// The text at `range` in `code`, if it is only hexadecimal digits.
/// `from_str_radix` alone would also take a leading `+`.
fn hex_digits<'a>(
    code: &'a str,
    range: Range<usize>,
    field: &'static str,
) -> Result<&'a str, CheatError> {
    let digits = code.get(range).unwrap_or_default();
    if digits.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        Ok(digits)
    } else {
        Err(CheatError::InvalidDigits {
            field,
            digits: digits.to_string(),
        })
    }
}

/// Reads the hexadecimal digits at `range` in `code`.
fn parse_hex(code: &str, range: Range<usize>, field: &'static str) -> Result<u16, CheatError> {
    let digits = hex_digits(code, range, field)?;
    u16::from_str_radix(digits, 16).map_err(|_| CheatError::InvalidDigits {
        field,
        digits: digits.to_string(),
    })
}

/// Reads the two hexadecimal digits at `range` in `code`.
fn parse_hex_byte(code: &str, range: Range<usize>, field: &'static str) -> Result<u8, CheatError> {
    let digits = hex_digits(code, range, field)?;
    u8::from_str_radix(digits, 16).map_err(|_| CheatError::InvalidDigits {
        field,
        digits: digits.to_string(),
    })
}

/// Parses an eight digit GameShark code such as `01FF56D3`, which makes a
/// byte of cartridge or work RAM read as another value.
///
/// The first byte selects the RAM bank, like SameBoy reads it: `01`, which
/// almost every code uses, applies to any bank, and otherwise the low digit
/// is the bank (Game Boy Color codes such as `91...` use work RAM bank 1).
pub(super) fn parse_gameshark_code(code: &str) -> Result<Cheat, CheatError> {
    if code.len() != 8 {
        return Err(CheatError::InvalidGameSharkLength);
    }

    let bank = parse_hex_byte(code, 0..2, "bank byte")?;
    let new_value = parse_hex_byte(code, 2..4, "new data byte")?;
    let address_low = parse_hex_byte(code, 4..6, "address byte")?;
    let address_high = parse_hex_byte(code, 6..8, "address byte")?;
    let address = u16::from_le_bytes([address_low, address_high]);

    if !(0xA000..=0xDFFF).contains(&address) {
        return Err(CheatError::InvalidAddress(address));
    }

    Ok(Cheat {
        address,
        new_value,
        compare_value: None,
        bank: (bank != 0x01).then_some(bank & 0x0F),
    })
}

/// Parses a Game Genie code such as `CED-56A` or `CED-56A-D50`, which
/// changes a byte of the cartridge ROM.
///
/// For a code `ABC-DEF-GHI`, `AB` is the new byte and `FCDE` is the address
/// with the top digit `F` inverted. The optional `G` and `I` hold the
/// encoded byte the ROM must contain for the cheat to apply; `H` is unused.
pub(super) fn parse_gamegenie_code(code: &str) -> Result<Cheat, CheatError> {
    if code.len() != 11 && code.len() != 7 {
        return Err(CheatError::InvalidGameGenieLength);
    }
    if !code.is_ascii() {
        return Err(CheatError::InvalidDigits {
            field: "code",
            digits: code.to_string(),
        });
    }
    let dashes_in_place = code
        .char_indices()
        .all(|(index, character)| (character == '-') == (index == 3 || index == 7));
    if !dashes_in_place {
        return Err(CheatError::InvalidGameGenieLength);
    }

    let digits = code.replace('-', "");
    let (digits, has_compare_value) = match digits.len() {
        6 => (digits, false),
        9 => {
            hex_digits(&digits, 7..8, "unused digit")?;
            (format!("{}{}", &digits[..7], &digits[8..]), true)
        }
        _ => return Err(CheatError::InvalidGameGenieLength),
    };

    let new_value = parse_hex_byte(&digits, 0..2, "new data byte")?;
    let address_low = parse_hex(&digits, 2..5, "address word")?;
    let address_high = parse_hex(&digits, 5..6, "address byte")?;
    let address = ((address_high << 12) | address_low) ^ 0xF000;

    if address > 0x7FFF {
        return Err(CheatError::InvalidAddress(address));
    }

    let compare_value = if has_compare_value {
        let encoded = parse_hex_byte(&digits, 6..8, "old data byte")?;
        Some(encoded.rotate_right(2) ^ 0xBA)
    } else {
        None
    };

    Ok(Cheat {
        address,
        new_value,
        compare_value,
        bank: None,
    })
}

/// Checks whether a GameShark code is valid without turning it on.
pub fn validate_gameshark_code(code: &str) -> Result<(), CheatError> {
    parse_gameshark_code(code).map(|_| ())
}

/// Checks whether a Game Genie code is valid without turning it on.
pub fn validate_gamegenie_code(code: &str) -> Result<(), CheatError> {
    parse_gamegenie_code(code).map(|_| ())
}

impl CheatState {
    /// No cheats turned on.
    pub(super) fn new() -> CheatState {
        CheatState {
            registered: Vec::new(),
        }
    }

    /// Turns on `cheat` under `cheat_id`. A cheat already registered under
    /// that id is replaced and keeps its place in the order.
    fn register(&mut self, cheat_id: &str, cheat: Cheat) -> Result<(), CheatError> {
        if let Some(registered) = self
            .registered
            .iter_mut()
            .find(|registered| registered.id == cheat_id)
        {
            registered.cheat = cheat;
            return Ok(());
        }
        if self.registered.len() >= CHEAT_LIMIT {
            return Err(CheatError::TooManyCheats { limit: CHEAT_LIMIT });
        }
        self.registered.push(RegisteredCheat {
            id: cheat_id.to_string(),
            cheat,
        });
        Ok(())
    }

    /// Turns off the cheat registered under `cheat_id`, if there is one.
    pub(crate) fn unregister(&mut self, cheat_id: &str) {
        self.registered
            .retain(|registered| registered.id != cheat_id);
    }

    /// Parses a GameShark `code` and turns it on under `cheat_id`.
    pub(crate) fn register_gameshark_cheat(
        &mut self,
        cheat_id: &str,
        code: &str,
    ) -> Result<(), CheatError> {
        let cheat = parse_gameshark_code(code)?;
        self.register(cheat_id, cheat)
    }

    /// Parses a Game Genie `code` and turns it on under `cheat_id`.
    pub(crate) fn register_gamegenie_cheat(
        &mut self,
        cheat_id: &str,
        code: &str,
    ) -> Result<(), CheatError> {
        let cheat = parse_gamegenie_code(code)?;
        self.register(cheat_id, cheat)
    }
}

impl AddressBus {
    /// The bank currently mapped at `address`: the cartridge RAM bank at
    /// 0xA000-0xBFFF, or the switchable work RAM bank at 0xD000-0xDFFF.
    /// 0xC000-0xCFFF always shows work RAM bank 0.
    fn ram_bank_for_address(&self, address: u16) -> u8 {
        match address {
            0xA000..=0xBFFF => self.cartridge.selected_ram_bank(),
            0xD000..=0xDFFF => self.working_ram_bank(),
            _ => 0,
        }
    }

    /// Returns the byte the game should see at `address`, which is
    /// `original_value` unless an active cheat replaces it.
    #[inline]
    pub(super) fn apply_cheats(&self, address: u16, original_value: u8) -> u8 {
        if self.cheats.registered.is_empty() {
            original_value
        } else {
            self.apply_registered_cheats(address, original_value)
        }
    }

    /// `apply_cheats` when there are cheats.
    fn apply_registered_cheats(&self, address: u16, original_value: u8) -> u8 {
        self.cheats
            .registered
            .iter()
            .map(|registered| &registered.cheat)
            .filter(|cheat| cheat.address == address)
            .find(|cheat| match (cheat.bank, cheat.compare_value) {
                (Some(bank), _) => bank == self.ram_bank_for_address(address),
                (None, Some(compare_value)) => original_value == compare_value,
                (None, None) => true,
            })
            .map_or(original_value, |cheat| cheat.new_value)
    }
}

#[cfg(test)]
mod tests {
    use crate::address_bus::constants::*;
    use crate::address_bus::effects::empty_cartridge_effects;
    use crate::address_bus::test_utils::*;
    use crate::model::Model;

    use super::*;

    #[test]
    fn should_parse_gameshark_code() {
        let gameshark_code = "01FF56D3";
        let cheat = parse_gameshark_code(gameshark_code).unwrap();
        assert_eq!(cheat.address, 0xD356);
        assert_eq!(cheat.new_value, 0xFF);
        assert_eq!(cheat.compare_value, None);
        assert_eq!(cheat.bank, None);
    }

    #[test]
    fn rejects_cheat_codes_with_a_sign_or_misplaced_dashes() {
        assert!(parse_gameshark_code("+1FF56D3").is_err());
        assert!(parse_gameshark_code("01FF+6D3").is_err());
        for code in [
            "CED-+6A",
            "CED56A-",
            "-CED56A",
            "CE-D56A",
            "CED-56AD50-",
            "CED-56A-D5-",
            "CED-56A+D50",
            "CED-56A-DZ0",
        ] {
            assert!(parse_gamegenie_code(code).is_err(), "{code}");
        }
        assert!(parse_gamegenie_code("CED-56A").is_ok());
        assert!(parse_gamegenie_code("CED-56A-D50").is_ok());
    }

    #[test]
    fn reads_the_work_ram_bank_from_a_game_boy_color_gameshark_code() {
        let cheat = parse_gameshark_code("92FF56D3").unwrap();
        assert_eq!(cheat.bank, Some(2));
    }

    fn cgb_address_bus_with_work_ram_bank(bank: u8) -> AddressBus {
        let mut address_bus = initialize_test_address_bus();
        address_bus.set_model(Model::CgbE);
        address_bus.poke(0xFF70, bank);
        address_bus
    }

    #[test]
    fn applies_a_bank_01_gameshark_code_whatever_bank_is_selected() {
        let mut address_bus = cgb_address_bus_with_work_ram_bank(3);
        address_bus
            .cheats_mut()
            .register_gameshark_cheat("cheat", "01FF56D3")
            .unwrap();
        assert_eq!(address_bus.peek(0xD356), 0xFF);
    }

    #[test]
    fn applies_a_work_ram_bank_code_only_while_that_bank_is_selected() {
        let mut address_bus = cgb_address_bus_with_work_ram_bank(2);
        address_bus
            .cheats_mut()
            .register_gameshark_cheat("cheat", "92FF56D3")
            .unwrap();
        assert_eq!(address_bus.peek(0xD356), 0xFF);

        address_bus.poke(0xFF70, 3);
        assert_ne!(address_bus.peek(0xD356), 0xFF);
    }

    #[test]
    fn treats_0xc000_to_0xcfff_as_work_ram_bank_0() {
        let mut address_bus = cgb_address_bus_with_work_ram_bank(2);
        address_bus
            .cheats_mut()
            .register_gameshark_cheat("cheat", "00FF56C3")
            .unwrap();
        assert_eq!(address_bus.peek(0xC356), 0xFF);
    }

    #[test]
    fn should_apply_gameshark_code() {
        let mut address_bus = initialize_test_address_bus();
        address_bus.set_model(Model::CgbE);

        let cheat = Cheat {
            address: 0xD356,
            new_value: 0xFF,
            compare_value: None,
            bank: Some(1),
        };

        address_bus
            .cheats_mut()
            .register("da9a7056-ad9b-4ca1-9049-688931b279a3", cheat)
            .unwrap();

        // Switch to working RAM bank 1
        address_bus.poke(0xFF70, 1);

        let byte = address_bus.peek(0xD356);

        assert_eq!(byte, 0xFF);
    }

    #[test]
    fn should_parse_gamegenie_code() {
        let gamegenie_code = "CED-56A-D50";
        let cheat = parse_gamegenie_code(gamegenie_code).unwrap();
        assert_eq!(cheat.new_value, 0xCE);
        assert_eq!(cheat.address, 0x5D56);
        assert_eq!(cheat.compare_value, Some(0x8E));
        assert_eq!(cheat.bank, None);
    }

    #[test]
    fn should_apply_gamegenie_code() {
        let mut address_bus = initialize_test_address_bus();
        address_bus.set_model(Model::CgbE);

        let mut test_rom = build_rom(CARTRIDGE_TYPE_MBC1_RAM, ROM_SIZE_64KB, RAM_SIZE_2KB);
        test_rom[0x5D56] = 0x8E;
        address_bus
            .load_rom(test_rom, empty_cartridge_effects())
            .unwrap();

        let cheat = Cheat {
            address: 0x5D56,
            new_value: 0xCE,
            compare_value: Some(0x8E),
            bank: None,
        };

        address_bus
            .cheats_mut()
            .register("da9a7056-ad9b-4ca1-9049-688931b279a3", cheat)
            .unwrap();

        let byte = address_bus.peek(0x5D56);

        assert_eq!(byte, 0xCE);
    }

    #[test]
    fn should_parse_gamegenie_code_without_compare_value() {
        let gamegenie_code = "CED-56A";
        let cheat = parse_gamegenie_code(gamegenie_code).unwrap();
        assert_eq!(cheat.new_value, 0xCE);
        assert_eq!(cheat.address, 0x5D56);
        assert_eq!(cheat.compare_value, None);
        assert_eq!(cheat.bank, None);
    }

    #[test]
    fn rejects_codes_that_are_not_ascii_without_panicking() {
        assert!(parse_gameshark_code("é123456").is_err());
        assert!(parse_gamegenie_code("é2-345").is_err());
        assert!(parse_gamegenie_code("CED-56é-D5").is_err());
    }

    #[test]
    fn rejects_seven_digit_game_genie_codes_without_dashes() {
        assert_eq!(
            parse_gamegenie_code("CED56AD").err(),
            Some(CheatError::InvalidGameGenieLength)
        );
    }

    #[test]
    fn the_first_cheat_turned_on_wins_when_two_change_the_same_byte() {
        let mut address_bus = cgb_address_bus_with_work_ram_bank(1);
        for (id, code) in [("a", "011156D3"), ("b", "012256D3"), ("c", "013356D3")] {
            address_bus
                .cheats_mut()
                .register_gameshark_cheat(id, code)
                .unwrap();
        }
        assert_eq!(address_bus.peek(0xD356), 0x11);

        address_bus.cheats_mut().unregister("a");
        assert_eq!(address_bus.peek(0xD356), 0x22);

        address_bus
            .cheats_mut()
            .register_gameshark_cheat("b", "014456D3")
            .unwrap();
        assert_eq!(address_bus.peek(0xD356), 0x44);
    }

    #[test]
    fn replacing_a_cheat_is_allowed_when_the_limit_is_reached() {
        let mut cheats = CheatState::new();
        for index in 0..CHEAT_LIMIT {
            cheats
                .register_gameshark_cheat(&index.to_string(), "01FF56D3")
                .unwrap();
        }
        assert!(cheats.register_gameshark_cheat("0", "01FF56D3").is_ok());
        assert!(cheats.register_gameshark_cheat("new", "01FF56D3").is_err());
    }
}

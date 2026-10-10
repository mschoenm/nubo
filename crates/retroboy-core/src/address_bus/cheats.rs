use crate::address_bus::AddressBus;
use std::collections::HashMap;
use std::io::{Error, ErrorKind, Result};

pub(super) struct Cheat {
    pub address: u16,
    pub new_data: u8,
    pub maybe_old_data: Option<u8>,
    pub maybe_bank: Option<u8>,
}

pub(crate) struct CheatState {
    registered: HashMap<String, Cheat>,
}

fn parse_hex_byte(slice: &str, field: &str) -> Result<u8> {
    u8::from_str_radix(slice, 16)
        .map_err(|_|
            Error::new(
                ErrorKind::InvalidInput,
                format!("Invalid {} byte: {}", field, slice)))
}

fn parse_hex_word(slice: &str, field: &str) -> Result<u16> {
    u16::from_str_radix(slice, 16)
        .map_err(|_|
            Error::new(
                ErrorKind::InvalidInput,
                format!("Invalid {} word: {}", field, slice)))
}

pub(super) fn parse_gameshark_code(gameshark_code: &str) -> Result<Cheat> {
    if gameshark_code.len() != 8 {
        Err(Error::new(
            ErrorKind::InvalidInput, 
            "Gameshark codes must be eight digits long."))
    }
    else {
        let parsed_bank = parse_hex_byte(&gameshark_code[..2], "bank")?;
        let parsed_new_data = parse_hex_byte(&gameshark_code[2..4], "new data")?;
        let parsed_addr_ls = parse_hex_byte(&gameshark_code[4..6], "address")?;
        let parsed_addr_ms = parse_hex_byte(&gameshark_code[6..8], "address")?;
    
        let address = ((parsed_addr_ms as u16) << 8) | (parsed_addr_ls as u16);

        if address < 0xA000 || address > 0xDFFF {
            Err(Error::new(
                ErrorKind::InvalidInput,
                format!("Invalid address: {}", address)))
        }
        else {
            Ok(Cheat {
                address,
                new_data: parsed_new_data,
                maybe_old_data: None,
                maybe_bank: Some(parsed_bank),
            }) 
        }
    }
}

pub(super) fn parse_gamegenie_code(gamegenie_code: &str) -> Result<Cheat> {
    if gamegenie_code.len() != 11 && gamegenie_code.len() != 7 {
        Err(Error::new(
            ErrorKind::InvalidInput, 
            "Game Genie codes must be either eleven or seven digits long."))
    }
    else {
        let without_dashes = gamegenie_code.replace("-", "");
        let stripped_code = if without_dashes.len() > 6 {
            format!("{}{}", &without_dashes[..7], &without_dashes[8..])
        }
        else {
            without_dashes
        };

        let parsed_new_data = parse_hex_byte(&stripped_code[..2], "new data")?;
        let parsed_addr_ls = parse_hex_word(&stripped_code[2..5], "address")?;
        let parsed_addr_ms = parse_hex_byte(&stripped_code[5..6], "address")?;
    
        let address = (((parsed_addr_ms as u16) << 12) | (parsed_addr_ls)) ^ 0xF000;

        if address > 0x7FFF {
            Err(Error::new(
                ErrorKind::InvalidInput,
                format!("Invalid address: {}", address)))
        }
        else {
            let maybe_old_data = if stripped_code.len() > 6 {
                let parsed_old_data = parse_hex_byte(&stripped_code[6..8], "old data")?;
                Some((parsed_old_data.rotate_right(2)) ^ 0xBA)
            }
            else {
                None
            };
    
            Ok(Cheat {
                address,
                new_data: parsed_new_data,
                maybe_old_data,
                maybe_bank: None,
            }) 
        }
    }
}

pub fn validate_gameshark_code(cheat_code: &str) -> Option<String> {
    match parse_gameshark_code(cheat_code) {
        Ok(_) => None,
        Err(error) => Some(error.to_string())
    }
}

pub fn validate_gamegenie_code(cheat_code: &str) -> Option<String> {
    match parse_gamegenie_code(cheat_code) {
        Ok(_) => None,
        Err(error) => Some(error.to_string())
    }
}

const CHEAT_LIMIT: usize = 10;

impl CheatState {
    pub(super) fn new() -> CheatState {
        CheatState {
            registered: HashMap::new(),
        }
    }

    fn register(&mut self, cheat_id: &str, cheat: Cheat) -> Option<String> {
        if self.registered.len() >= CHEAT_LIMIT {
            Some(format!("You cannot register more than {CHEAT_LIMIT} cheats at a time."))
        }
        else {
            self.registered.insert(cheat_id.to_string(), cheat); 
            None
        }
    }

    pub(crate) fn unregister(&mut self, cheat_id: &str) {
        self.registered.remove(cheat_id);
    }

    pub(crate) fn register_gameshark_cheat(&mut self, cheat_id: &str, cheat_code: &str) -> Option<String> {
        match parse_gameshark_code(cheat_code) {
            Ok(cheat) => {
                self.register(cheat_id, cheat)
            },
            Err(error) => Some(error.to_string())
        }
    }

    pub(crate) fn register_gamegenie_cheat(&mut self, cheat_id: &str, cheat_code: &str) -> Option<String> {
        match parse_gamegenie_code(cheat_code) {
            Ok(cheat) => {
                self.register(cheat_id, cheat)
            },
            Err(error) => Some(error.to_string())
        }
    }

    pub(super) fn registered_cheats(&self) -> &HashMap<String, Cheat> {
        &self.registered
    }
}

impl AddressBus {
    fn get_ram_bank_for_address(&self, address: u16) -> u8 {
        match address & 0xF000 {
            0xA000..=0xBFFF => self.cartridge_mapper().get_ram_bank(),
            0xC000..=0xDFFF => self.get_working_ram_bank(),
            _ => 0
        }
    }

    pub(super) fn apply_cheat_if_needed(&self, address: u16, old_data: u8) -> u8 {
        let mut maybe_found_cheat = None;

        for (_, cheat) in self.cheats.registered_cheats() {
            if cheat.address == address {
                let apply_cheat = if cheat.maybe_bank.is_some() {
                    let cheat_bank = cheat.maybe_bank.unwrap();
                    let current_bank = self.get_ram_bank_for_address(address);
                    cheat_bank == current_bank
                }
                else if cheat.maybe_bank.is_none() && cheat.maybe_old_data.is_some() {
                    let cheat_old_data = cheat.maybe_old_data.unwrap();
                    old_data == cheat_old_data
                }
                else {
                    true
                };
            
                if apply_cheat {
                    maybe_found_cheat = Some(cheat);
                    break;
                }
            }
        };

        maybe_found_cheat
            .map(|cheat| cheat.new_data)
            .unwrap_or_else(|| old_data)
    }
}

#[cfg(test)]
mod tests {
    use crate::address_bus::MemoryMapped;
    use crate::address_bus::constants::*;
    use crate::address_bus::test_utils::*;
    use crate::address_bus::effects::empty_cartridge_effects;

    use super::*;

    #[test]
    fn should_parse_gameshark_code() {
        let gameshark_code = "01FF56D3";
        let cheat = parse_gameshark_code(gameshark_code).unwrap();
        assert_eq!(cheat.address, 0xD356);
        assert_eq!(cheat.new_data, 0xFF);
        assert_eq!(cheat.maybe_old_data, None);
        assert_eq!(cheat.maybe_bank, Some(1));
    }

    #[test]
    fn should_apply_gameshark_code() {
        let mut address_bus = initialize_test_address_bus();
        address_bus.set_cgb_mode(true);

        let cheat = Cheat {
            address: 0xD356,
            new_data: 0xFF,
            maybe_old_data: None,
            maybe_bank: Some(1),
        };

        address_bus.cheats_mut().register("da9a7056-ad9b-4ca1-9049-688931b279a3", cheat);

        // Switch to working RAM bank 1
        address_bus.write_byte(0xFF70, 1);
        
        let byte = address_bus.read_byte(0xD356);

        assert_eq!(byte, 0xFF);
    }

    #[test]
    fn should_parse_gamegenie_code() {
        let gamegenie_code = "CED-56A-D50";
        let cheat = parse_gamegenie_code(gamegenie_code).unwrap();
        assert_eq!(cheat.new_data, 0xCE);
        assert_eq!(cheat.address, 0x5D56);
        assert_eq!(cheat.maybe_old_data, Some(0x8E));
        assert_eq!(cheat.maybe_bank, None);
    }

    #[test]
    fn should_apply_gamegenie_code() {
        let mut address_bus = initialize_test_address_bus();
        address_bus.set_cgb_mode(true);

        let mut test_rom = build_rom(CART_TYPE_MBC1_WITH_RAM, ROM_SIZE_64KB, RAM_SIZE_2KB);
        test_rom[0x5D56] = 0x8E;
        address_bus.load_rom_buffer(test_rom, empty_cartridge_effects()).unwrap();

        let cheat = Cheat {
            address: 0x5D56,
            new_data: 0xCE,
            maybe_old_data: Some(0x8E),
            maybe_bank: None,
        };

        address_bus.cheats_mut().register("da9a7056-ad9b-4ca1-9049-688931b279a3", cheat);

        let byte = address_bus.read_byte(0x5D56);

        assert_eq!(byte, 0xCE);
    }

    #[test]
    fn should_parse_gamegenie_code_without_compare_value() {
        let gamegenie_code = "CED-56A";
        let cheat = parse_gamegenie_code(gamegenie_code).unwrap();
        assert_eq!(cheat.new_data, 0xCE);
        assert_eq!(cheat.address, 0x5D56);
        assert_eq!(cheat.maybe_old_data, None);
        assert_eq!(cheat.maybe_bank, None);
    }
}
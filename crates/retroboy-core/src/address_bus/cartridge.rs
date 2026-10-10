use core::panic;
use std::io;

use crate::address_bus::constants::*;
use crate::address_bus::effects::CartridgeEffects;
use crate::address_bus::huc1::HUC1CartridgeMapper;
use crate::address_bus::mbc1::MBC1CartridgeMapper;
use crate::address_bus::mbc3::MBC3CartridgeMapper;
use crate::address_bus::mbc5::MBC5CartridgeMapper;
use crate::address_bus::mbc_rom_only::MBCRomOnlyCartridgeMapper;
use crate::serializable::Serializable;
use std::fmt::Debug;

#[derive(Debug, Clone)]
pub struct CartridgeHeader {
    pub sgb_support: bool,
    pub type_code: u8,
    pub max_banks: u16,
    pub max_ram_banks: u8,
    pub title: String,
    pub has_battery: bool
}

#[derive(Debug)]
pub(crate) struct Cartridge {
    pub rom: Vec<u8>,
    pub ram: Vec<u8>,
    pub header: CartridgeHeader,
    pub effects: Box<dyn CartridgeEffects>
}

pub(crate) trait CartridgeMapper: Debug + Serializable {
    fn read_rom(&self, address: u16) -> u8;
    fn write_rom(&mut self, address: u16, value: u8);
    fn read_ram(&self, address: u16) -> u8;
    fn write_ram(&mut self, address: u16, value: u8);

    fn get_cartridge(&self) -> &Cartridge;
    fn set_cartridge_ram(&mut self, ram: Vec<u8>);
    
    fn get_ram_bank(&self) -> u8;

    fn title(&self) -> String {
        self.get_cartridge().header.title.clone()
    }
}

const SUPPORTED_CARTRIDGE_TYPES: [u8; 16] = [CART_TYPE_ROM_ONLY,
    CART_TYPE_MBC1,
    CART_TYPE_MBC1_WITH_RAM,
    CART_TYPE_MBC1_WITH_RAM_PLUS_BATTERY,
    CART_TYPE_MBC3_TIMER_BATTERY,
    CART_TYPE_MBC3_TIMER_RAM_BATTERY,
    CART_TYPE_MBC3,
    CART_TYPE_MBC3_RAM,
    CART_TYPE_MBC3_RAM_BATTERY,
    CART_TYPE_MBC5,
    CART_TYPE_MBC5_RAM,
    CART_TYPE_MBC5_RAM_BATTERY,
    CART_TYPE_MBC5_RUMBLE,
    CART_TYPE_MBC5_RUMBLE_RAM,
    CART_TYPE_MBC5_RUMBLE_RAM_BATTERY,
    CART_TYPE_HUC1_RAM_BATTERY];

pub(super) fn initialize_cartridge(effects: Box<dyn CartridgeEffects>) -> Cartridge {
    Cartridge {
        rom: Vec::new(),
        ram: Vec::new(),
        header: CartridgeHeader {
            sgb_support: false,
            type_code: 0,
            max_banks: 0,
            max_ram_banks: 0,
            title: String::from(""),
            has_battery: false
        },
        effects
    }
}

pub(super) fn initialize_cartridge_mapper(effects: Box<dyn CartridgeEffects>) -> Box<dyn CartridgeMapper> {
    Box::new(MBCRomOnlyCartridgeMapper::new(initialize_cartridge(effects)))
}

fn cartridge_type_supported(type_code: u8) -> bool {
    SUPPORTED_CARTRIDGE_TYPES.contains(&type_code)
}

pub(super) fn as_max_banks(rom_size_index: u8) -> u16 {
    (2 as u16).pow(rom_size_index as u32 + 1)
}

pub(super) fn as_max_ram_banks(ram_size: u32) -> u8 {
    match ram_size {
        0 => 0,
        0x800 => 1,
        _ => (ram_size / 0x2000) as u8
    }
}

fn is_mbc_rom_only(type_code: u8) -> bool {
    type_code == CART_TYPE_ROM_ONLY
}

fn is_mbc1(type_code: u8) -> bool {
    matches!(type_code, CART_TYPE_MBC1
        | CART_TYPE_MBC1_WITH_RAM
        | CART_TYPE_MBC1_WITH_RAM_PLUS_BATTERY)
}

fn is_mbc3(type_code: u8) -> bool {
    matches!(type_code, CART_TYPE_MBC3
        | CART_TYPE_MBC3_RAM
        | CART_TYPE_MBC3_RAM_BATTERY
        | CART_TYPE_MBC3_TIMER_BATTERY
        | CART_TYPE_MBC3_TIMER_RAM_BATTERY)
}

fn is_mbc5(type_code: u8) -> bool {
    matches!(type_code, CART_TYPE_MBC5
        | CART_TYPE_MBC5_RAM
        | CART_TYPE_MBC5_RAM_BATTERY
        | CART_TYPE_MBC5_RUMBLE
        | CART_TYPE_MBC5_RUMBLE_RAM
        | CART_TYPE_MBC5_RUMBLE_RAM_BATTERY)
}

fn is_huc1(type_code: u8) -> bool {
    type_code == CART_TYPE_HUC1_RAM_BATTERY
}

fn is_battery_backed(type_code: u8) -> bool {
    matches!(type_code, CART_TYPE_MBC1_WITH_RAM_PLUS_BATTERY
        | CART_TYPE_MBC3_TIMER_BATTERY
        | CART_TYPE_MBC3_TIMER_RAM_BATTERY
        | CART_TYPE_MBC3_RAM_BATTERY
        | CART_TYPE_MBC5_RAM_BATTERY
        | CART_TYPE_HUC1_RAM_BATTERY)
}

fn set_ram_size(cartridge: &mut Cartridge) {
    let ram_size_index = cartridge.rom[RAM_SIZE_ADDRESS];
    let ram_size = match ram_size_index {
        0x0 => 0,
        0x1 => 0x800,
        0x2 => 0x2000,
        0x3 => 0x8000,
        0x4 => 0x20000,
        0x5 => 0x10000,
        _ => panic!("Unsupported RAM size index: {}", ram_size_index),
    };
    cartridge.ram.resize(ram_size as usize, 0);
}

fn is_cgb_compatability_flag(index: usize, byte: u8) -> bool {
    index == CGB_COMPATABILITY_INDEX && (byte == 0xC0 || byte == 0x80)
}

pub(super) fn convert_cartridge_type_to_text(type_code: u8) -> String {
    match type_code {
        0x00 => "ROM ONLY",
        0x01 => "MBC1",
        0x02 => "MBC1+RAM",
        0x03 => "MBC1+RAM+BATTERY",
        0x05 => "MBC2",
        0x06 => "MBC2+BATTERY",
        0x08 => "ROM+RAM",
        0x09 => "ROM+RAM+BATTERY",
        0x0B => "MMM01",
        0x0C => "MMM01+RAM",
        0x0D => "MMM01+RAM+BATTERY",
        0x0F => "MBC3+TIMER+BATTERY",
        0x10 => "MBC3+TIMER+RAM+BATTERY",
        0x11 => "MBC3",
        0x12 => "MBC3+RAM",
        0x13 => "MBC3+RAM+BATTERY",
        0x19 => "MBC5",
        0x1A => "MBC5+RAM",
        0x1B => "MBC5+RAM+BATTERY",
        0x1C => "MBC5+RUMBLE",
        0x1D => "MBC5+RUMBLE+RAM",
        0x1E => "MBC5+RUMBLE+RAM+BATTERY",
        0x20 => "MBC6",
        0x22 => "MBC7+SENSOR+RUMBLE+RAM+BATTERY",
        0xFC => "POCKET CAMERA",
        0xFD => "BANDAI TAMA5",
        0xFE => "HuC3",
        0xFF => "HuC1+RAM+BATTERY",
        _ => "UNKNOWN"
    }.to_string()
}

fn as_mapper(cartridge: Cartridge, type_code: u8) -> Box<dyn CartridgeMapper> {
    if is_mbc_rom_only(type_code) {
        Box::new(MBCRomOnlyCartridgeMapper::new(cartridge))
    } else if is_huc1(type_code) {
        Box::new(HUC1CartridgeMapper::new(cartridge))
    } else if is_mbc1(type_code) {
        Box::new(MBC1CartridgeMapper::new(cartridge))
    } else if is_mbc3(type_code) {
        Box::new(MBC3CartridgeMapper::new(cartridge))
    } else if is_mbc5(type_code) {
        Box::new(MBC5CartridgeMapper::new(cartridge))
    } else {
        panic!("Unsupported cartridge type: {}", type_code);
    }
}

pub(super) fn load_rom_buffer(buffer: Vec<u8>, effects: Box<dyn CartridgeEffects>) -> io::Result<Box<dyn CartridgeMapper>> {
    if buffer.len() > ENTRY_POINT_ADDRESS {
        let type_code = buffer[CARTRIDGE_TYPE_ADDRESS];
        let sgb_support = buffer[SGB_SUPPORT_ADDRESS] == 0x03;
        let rom_size = buffer[ROM_SIZE_ADDRESS];

        let title_bytes = &buffer[TITLE_START_ADDRESS..=TITLE_END_ADDRESS];
        let title = title_bytes
            .iter()
            .enumerate()
            .take_while(|&(i, &b)| !is_cgb_compatability_flag(i, b) && b != 0x00)
            .map(|(_, &b)| b as char)
            .collect::<String>();

        if cartridge_type_supported(type_code) {
            let mut cartridge = Cartridge {
                rom: buffer,
                ram: Vec::new(),
                header: CartridgeHeader {
                    sgb_support,
                    type_code,
                    max_banks: as_max_banks(rom_size),
                    max_ram_banks: 0,
                    title,
                    has_battery: is_battery_backed(type_code)
                },
                effects
            };

            let maybe_loaded_ram = cartridge.effects.load_ram(&cartridge.header.title);
            if maybe_loaded_ram.is_some() {
                cartridge.ram = maybe_loaded_ram.unwrap();
            }
            else {
                set_ram_size(&mut cartridge);
            }

            cartridge.header.max_ram_banks = as_max_ram_banks(cartridge.ram.len() as u32);

            let mapper = as_mapper(cartridge, type_code);

            Ok(mapper)
        } else {
            let given_cartridge_type = convert_cartridge_type_to_text(type_code);

            let error_message = format!(r#"Sorry, but Retro Boy currently only supports
                ROM-only, MBC1, MBC3, MBC5, or HuC1 cartridges.
                The cartridge you provided is of type {}."#, given_cartridge_type);

            Err(io::Error::new(io::ErrorKind::Other, error_message))
        }
    } else {
        let error_message = "Buffer is too small to contain a valid ROM.";
        Err(io::Error::new(io::ErrorKind::Other, error_message))
    }
}

#[cfg(test)]
pub(crate) mod test_utils {
    use super::*;
    use crate::address_bus::effects::empty_cartridge_effects;
    use crate::address_bus::test_utils::*;

    pub(crate) fn build_cartridge_mapper(cartridge_type: u8, rom_size_index: u8, ram_size_index: u8) -> Box<dyn CartridgeMapper> {
        let rom_buffer = build_rom(cartridge_type, rom_size_index, ram_size_index);
        load_rom_buffer(rom_buffer, empty_cartridge_effects()).unwrap()
    }

    pub(crate) fn build_cartridge_mapper_with_effects(cartridge_type: u8, rom_size_index: u8, ram_size_index: u8, cartridge_effects: Box<dyn CartridgeEffects>) -> Box<dyn CartridgeMapper> {
        let rom_buffer = build_rom(cartridge_type, rom_size_index, ram_size_index);
        load_rom_buffer(rom_buffer, cartridge_effects).unwrap()
    }
}
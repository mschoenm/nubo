use crate::address_bus::bank_utils::{banked_read, banked_write};
use crate::address_bus::cartridge::{Cartridge, CartridgeMapper};
use crate::address_bus::constants::*;
use crate::serializable::Serializable;
use serializable_derive::Serializable;
use std::io::{Read, Write};

#[derive(Debug, Serializable)]
pub struct RTCState {
    pub milliseconds: u16,
    pub seconds: u8,
    pub minutes: u8,
    pub hours: u8,
    pub days: u16,
    pub base_timestamp: f64,
    pub halted: bool,
    pub day_carry: bool,
}

#[derive(Debug)]
pub(super) struct MBC3CartridgeMapper {
    cartridge: Cartridge,
    rom_bank_number: u8,
    ram_rtc_enabled: bool,
    ram_rtc_selection: u8,
    rtc: RTCState,
    rtc_latch: u8,
}

const INVALID_MAX_SECONDS: u8 = 63;
const INVALID_MAX_MINUTES: u8 = 63;
const INVALID_MAX_HOURS: u8 = 31;

fn ram_supported(cartridge: &Cartridge) -> bool {
    matches!(cartridge.header.type_code, CART_TYPE_MBC3_RAM | CART_TYPE_MBC3_RAM_BATTERY | CART_TYPE_MBC3_TIMER_RAM_BATTERY)
}

fn timer_supported(cartridge: &Cartridge) -> bool {
    matches!(cartridge.header.type_code, CART_TYPE_MBC3_TIMER_RAM_BATTERY | CART_TYPE_MBC3_TIMER_BATTERY)
}

fn battery_supported(cartridge: &Cartridge) -> bool {
    matches!(cartridge.header.type_code, CART_TYPE_MBC3_RAM_BATTERY | CART_TYPE_MBC3_TIMER_RAM_BATTERY | CART_TYPE_MBC3_TIMER_BATTERY)
}

impl MBC3CartridgeMapper {
    pub(super) fn new(cartridge: Cartridge) -> Self {
        let rtc_state_key = format!("{}-rtc", cartridge.header.title);
        let maybe_rtc_state = cartridge.effects.load_rtc_state(&rtc_state_key);
        MBC3CartridgeMapper {
            cartridge,
            rom_bank_number: 1,
            ram_rtc_enabled: false,
            ram_rtc_selection: 0,
            rtc: maybe_rtc_state.unwrap_or_else(|| RTCState {
                milliseconds: 0,
                seconds: 0,
                minutes: 0,
                hours: 0,
                days: 0,
                base_timestamp: 0.0,
                halted: false,
                day_carry: false,
            }),
            rtc_latch: 0xFF,
        }
    }

    fn update_rtc_time_registers(&mut self, elapsed_ms: f64) {
        let mut new_milliseconds = self.rtc.milliseconds as u64 + elapsed_ms as u64;
        let mut new_seconds = self.rtc.seconds as u64;
        let mut new_minutes = self.rtc.minutes as u64;
        let mut new_hours = self.rtc.hours as u64;
        let mut new_days = self.rtc.days as u64;

        let mut invalid_rollover = false;

        new_seconds += new_milliseconds / 1000;
        new_milliseconds %= 1000;

        if self.rtc.seconds < 60 {
            new_minutes += new_seconds / 60;
            new_seconds %= 60;
        }
        else if new_seconds > INVALID_MAX_SECONDS as u64 {
            invalid_rollover = true;
            new_seconds = 0;
        }
 
        if self.rtc.minutes < 60 && !invalid_rollover {
            new_hours += new_minutes / 60;
            new_minutes %= 60;
        } else if new_minutes > INVALID_MAX_MINUTES as u64 {
            invalid_rollover = true;
            new_minutes = 0;
        }

        if self.rtc.hours < 24 && !invalid_rollover {
            new_days += new_hours / 24;
            new_hours %= 24;
        } else if new_hours > INVALID_MAX_HOURS as u64 {
            invalid_rollover = true;
            new_hours = 0;
        }

        if new_days >= 512 && !invalid_rollover {
            new_days %= 512;
            self.rtc.day_carry = true; 
        }

        self.rtc.milliseconds = new_milliseconds as u16;
        self.rtc.seconds = new_seconds as u8;
        self.rtc.minutes = new_minutes as u8;
        self.rtc.hours = new_hours as u8;
        self.rtc.days = new_days as u16; 
    }

    fn save_rtc_state(&self) {
        let key = format!("{}-rtc", self.cartridge.header.title);
        self.cartridge.effects.save_rtc_state(&key, &self.rtc);
    }

    fn save_ram(&self) {
        self.cartridge.effects.save_ram(&self.cartridge.header.title, &self.cartridge.ram);
    }
}

impl CartridgeMapper for MBC3CartridgeMapper {
    fn read_rom(&self, address: u16) -> u8 {
        match address {
            0x0000..=0x3FFF =>
                self.cartridge.rom[address as usize],
            0x4000..=0x7FFF => {
                banked_read(&self.cartridge.rom, 0x4000, address, self.rom_bank_number as u16)
            },
            _ => panic!("Invalid ROM address: {:#X}", address),
        }
    }

    fn write_rom(&mut self, address: u16, value: u8) {
        match address {
            0x0000..=0x1FFF => {
                if ram_supported(&self.cartridge) || timer_supported(&self.cartridge) {
                    self.ram_rtc_enabled = (value & 0xF) == 0x0A;
                }
            },
            0x2000..=0x3FFF => {
                let value = if value == 0 { 1 } else { value };
                self.rom_bank_number = value & 0x7F;
            },
            0x4000..=0x5FFF => {
                if value <= 0x03 || (value >= 0x08 && value <= 0x0C) {
                    self.ram_rtc_selection = value;
                }
            },
            0x6000..=0x7FFF => {
                if timer_supported(&self.cartridge) {
                    if self.rtc_latch == 0x00 && value == 0x01 {
                        let current_time = self.cartridge.effects.current_time_millis();

                        if !self.rtc.halted {
                            let elapsed_ms = current_time - self.rtc.base_timestamp;
                    
                            if elapsed_ms > 0.0 {
                                self.update_rtc_time_registers(elapsed_ms);
                                self.rtc.base_timestamp = current_time;
                                self.save_rtc_state();
                            } 
                        }
                    }
                    self.rtc_latch = value;
                }
            },
            _ => panic!("Invalid ROM address: {:#X}", address),
        }
    }
    
    fn read_ram(&self, address: u16) -> u8 {
        if self.ram_rtc_enabled {
            let ram_rtc_selection = self.ram_rtc_selection;
    
            match ram_rtc_selection {
                0x00..=0x03 if ram_supported(&self.cartridge) => {
                    banked_read(&self.cartridge.ram, 0x2000, address, self.ram_rtc_selection as u16)
                },
                0x08..=0x0C if timer_supported(&self.cartridge) => {
                    match self.ram_rtc_selection {
                        0x08 => self.rtc.seconds,
                        0x09 => self.rtc.minutes,
                        0x0A => self.rtc.hours,
                        0x0B => (self.rtc.days & 0xFF) as u8, 
                        0x0C => {
                            let mut value = (self.rtc.days >> 8) as u8;
                            if self.rtc.halted {
                                value |= 0x40;
                            }
                            if self.rtc.day_carry {
                                value |= 0x80;
                            }
                            value
                        }
                        _ => 0xFF,
                    }
                }
                _ => 0xFF,
            }
        } else {
            0xFF
        }
    }

    fn write_ram(&mut self, address: u16, value: u8) {
        if self.ram_rtc_enabled {
            let ram_rtc_selection = self.ram_rtc_selection;
    
            match ram_rtc_selection {
                0x00..=0x03 if ram_supported(&self.cartridge) => {
                    banked_write(&mut self.cartridge.ram, 0x2000, address, self.ram_rtc_selection as u16, value);
                    if battery_supported(&self.cartridge) {
                        self.save_ram();
                    }
                },
                0x08..=0x0C if timer_supported(&self.cartridge) => {
                    match self.ram_rtc_selection {
                        0x08 => self.rtc.seconds = value & INVALID_MAX_SECONDS,
                        0x09 => self.rtc.minutes = value & INVALID_MAX_MINUTES,
                        0x0A => self.rtc.hours = value & INVALID_MAX_HOURS,
                        0x0B => self.rtc.days = (self.rtc.days & 0x100) | (value & 0xFF) as u16,
                        0x0C => {
                            self.rtc.days = (self.rtc.days & 0xFF) | ((value as u16 & 0x01) << 8);
                            self.rtc.halted = value & 0x40 != 0;
                            self.rtc.day_carry = value & 0x80 != 0;
                        }
                        _ => {}
                    }
            
                    if self.ram_rtc_selection >= 0x08 && self.ram_rtc_selection <= 0x0C {
                        let current_time = self.cartridge.effects.current_time_millis();
                        
                        if !self.rtc.halted {
                            let elapsed_ms = current_time - self.rtc.base_timestamp;
                            if elapsed_ms > 0.0 {
                                self.update_rtc_time_registers(elapsed_ms);
                            }
                        }
                    
                        self.rtc.base_timestamp = current_time;
                        self.save_rtc_state();
                    }
                }
                _ => {}
            }
        }
    }

    fn get_cartridge(&self) -> &Cartridge {
        &self.cartridge
    }

    fn set_cartridge_ram(&mut self, ram: Vec<u8>) {
        self.cartridge.ram = ram;
    }

    fn get_ram_bank(&self) -> u8 {
        self.ram_rtc_selection
    }
}

impl Serializable for MBC3CartridgeMapper {
    fn serialize(&self, writer: &mut dyn Write)-> std::io::Result<()> {
        self.cartridge.ram.serialize(writer)?;
        self.rom_bank_number.serialize(writer)?;
        self.ram_rtc_enabled.serialize(writer)?;
        self.ram_rtc_selection.serialize(writer)?;
        self.rtc.serialize(writer)?;
        self.rtc_latch.serialize(writer)?;
        Ok(())
    }

    fn deserialize(&mut self, reader: &mut dyn Read)-> std::io::Result<()> {
        self.cartridge.ram.deserialize(reader)?;
        self.rom_bank_number.deserialize(reader)?;
        self.ram_rtc_enabled.deserialize(reader)?;
        self.ram_rtc_selection.deserialize(reader)?;
        self.rtc.deserialize(reader)?;
        self.rtc_latch.deserialize(reader)?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use crate::address_bus::cartridge::*;
    use crate::address_bus::cartridge::test_utils::*;
    use crate::address_bus::constants::*;
    use crate::address_bus::effects::{CartridgeEffects, empty_cartridge_effects};
    use crate::address_bus::test_utils::*;
    use super::*;

    struct FakeCartridgeEffects;

    impl CartridgeEffects for FakeCartridgeEffects {
        fn current_time_millis(&self) -> f64 {
            0.0
        }
    
        fn save_rtc_state(&self, _: &str, _: &RTCState) {}
    
        fn load_rtc_state(&self, _: &str) -> Option<RTCState> {
            Some(RTCState {
                milliseconds: 0,
                seconds: 10,
                minutes: 10,
                hours: 10,
                days: 0,
                base_timestamp: 0.0,
                halted: false,
                day_carry: false
            })
        }

        fn load_ram(&self, _: &str) -> Option<Vec<u8>> {
            None
        }

        fn save_ram(&self, _: &str, _: &[u8]) {}
    }

    fn fake_cartridge_effects() -> Box<dyn CartridgeEffects> {
        Box::new(FakeCartridgeEffects {})
    }

    #[test]
    fn reads_from_rom_bank_0() {
        let mut rom = build_rom(CART_TYPE_MBC3, ROM_SIZE_64KB, RAM_SIZE_2KB);
        rom[0] = 0xB1;
        rom[1] = 0xD2;
        let mapper = load_rom_buffer(rom, empty_cartridge_effects()).unwrap(); 

        let byte = mapper.read_rom(0x1);
        assert_eq!(byte, 0xD2);      
    }

    #[test]
    fn reads_from_rom_bank_2() {
        let mut rom = build_rom(CART_TYPE_MBC3, ROM_SIZE_64KB, RAM_SIZE_2KB);
        rom[0x8000] = 0xB1;
        rom[0x8001] = 0xD2;
        let mut mapper = load_rom_buffer(rom, empty_cartridge_effects()).unwrap(); 

        mapper.write_rom(0x2000, 2);

        let byte = mapper.read_rom(0x4001);
        assert_eq!(byte, 0xD2);
    }

    #[test]
    fn reads_from_ram_bank_1() {
        let mut mapper = build_cartridge_mapper(CART_TYPE_MBC3_RAM, ROM_SIZE_64KB, RAM_SIZE_32KB);

        let mut ram = vec![0; 0x8000];
        ram[0x2001] = 0xDD;
        mapper.set_cartridge_ram(ram);

        // Enables RAM
        mapper.write_rom(0x0000, 0xA);

        // Sets RAM bank to 1
        mapper.write_rom(0x4000, 0x1);

        let byte = mapper.read_ram(0x0001);
        assert_eq!(byte, 0xDD);
    }

    #[test]
    fn does_not_read_from_ram_if_not_supported() {
        let mut mapper = build_cartridge_mapper(CART_TYPE_MBC3_TIMER_BATTERY, ROM_SIZE_64KB, RAM_SIZE_32KB);

        let mut ram = vec![0; 0x8000];
        ram[0x2001] = 0xDD;
        mapper.set_cartridge_ram(ram);

        // Enables RAM
        mapper.write_rom(0x0000, 0xA);

        // Sets RAM bank to 1
        mapper.write_rom(0x4000, 0x1);

        let byte = mapper.read_ram(0x0001);
        assert_eq!(byte, 0xFF);
    }

    #[test]
    fn writes_to_ram_bank_1() {
        let mut mapper = build_cartridge_mapper(CART_TYPE_MBC3_RAM, ROM_SIZE_64KB, RAM_SIZE_32KB);

        // Enables RAM
        mapper.write_rom(0x0000, 0xA);

        // Sets RAM bank to 1
        mapper.write_rom(0x4000, 0x1);

        mapper.write_ram(0x0001, 0xDD);

        let byte = mapper.read_ram(0x001);
        assert_eq!(byte, 0xDD);
    }

    #[test]
    fn does_not_write_to_ram_if_not_supported() {
        let mut mapper = build_cartridge_mapper(CART_TYPE_MBC3_TIMER_BATTERY, ROM_SIZE_64KB, RAM_SIZE_32KB);

        // Enables RAM
        mapper.write_rom(0x0000, 0xA);

        // Sets RAM bank to 1
        mapper.write_rom(0x4000, 0x1);

        mapper.write_ram(0x0001, 0xDD);

        let byte = mapper.read_ram(0x001);
        assert_eq!(byte, 0xFF);
    }

    #[test]
    fn changes_rom_bank_number_to_bank_1_when_writing_0() {
        let mut rom = build_rom(CART_TYPE_MBC3, ROM_SIZE_64KB, RAM_SIZE_2KB);
        rom[0x4000] = 0xB1;
        rom[0x4001] = 0xD2;
        let mut mapper = load_rom_buffer(rom, empty_cartridge_effects()).unwrap(); 

        mapper.write_rom(0x2000, 2);

        mapper.write_rom(0x2000, 0);

        let byte = mapper.read_rom(0x4001);
        assert_eq!(byte, 0xD2);
    }

    #[test]
    fn reads_from_minutes_rtc_register() {
        let mut mapper = build_cartridge_mapper_with_effects(CART_TYPE_MBC3_TIMER_RAM_BATTERY,
            ROM_SIZE_64KB,
            RAM_SIZE_2KB,
            fake_cartridge_effects());

        mapper.write_rom(0x0000, 0xA);

        mapper.write_rom(0x4000, 0x9);

        // Latch RTC
        mapper.write_rom(0x6000, 0x0);
        mapper.write_rom(0x6000, 0x1);

        let byte = mapper.read_ram(0x0000);
        assert_eq!(byte, 0x0A);
    }

    #[test]
    fn does_not_read_from_minutes_rtc_register_if_not_supported() {
        let mut mapper = build_cartridge_mapper_with_effects(CART_TYPE_MBC3_RAM_BATTERY,
            ROM_SIZE_64KB,
            RAM_SIZE_2KB,
            fake_cartridge_effects());

        mapper.write_rom(0x0000, 0xA);

        mapper.write_rom(0x4000, 0x9);

        // Latch RTC
        mapper.write_rom(0x6000, 0x0);
        mapper.write_rom(0x6000, 0x1);

        let byte = mapper.read_ram(0x0000);
        assert_eq!(byte, 0xFF);
    }

    #[test]
    fn writes_to_minutes_rtc_register() {
        let mut mapper = build_cartridge_mapper_with_effects(CART_TYPE_MBC3_TIMER_RAM_BATTERY,
            ROM_SIZE_64KB,
            RAM_SIZE_2KB,
            fake_cartridge_effects());

        mapper.write_rom(0x0000, 0xA);

        mapper.write_rom(0x4000, 0x9);

        // Latch RTC
        mapper.write_rom(0x6000, 0x0);
        mapper.write_rom(0x6000, 0x1);

        mapper.write_ram(0x0000, 0x2);
        let byte = mapper.read_ram(0x0000);
        assert_eq!(byte, 0x02);
    }

    #[test]
    fn does_not_write_to_minutes_rtc_register_if_not_supported() {
        let mut mapper = build_cartridge_mapper_with_effects(CART_TYPE_MBC3_RAM_BATTERY,
            ROM_SIZE_64KB,
            RAM_SIZE_2KB,
            fake_cartridge_effects());

        mapper.write_rom(0x0000, 0xA);

        mapper.write_rom(0x4000, 0x9);

        // Latch RTC
        mapper.write_rom(0x6000, 0x0);
        mapper.write_rom(0x6000, 0x1);

        mapper.write_ram(0x0000, 0x2);
        let byte = mapper.read_ram(0x0000);
        assert_eq!(byte, 0xFF);
    }
}
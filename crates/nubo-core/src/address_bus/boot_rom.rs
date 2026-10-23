//! The boot ROM: the small program built into the console that shows the logo
//! and then hands control to the cartridge by writing to 0xFF50. Leaving it
//! puts the hardware into the state each model is in at that moment.

use crate::address_bus::AddressBus;
use crate::bios::{CGB_BOOT, DMG_BOOT};
use crate::model::Model;

/// LY and dot the PPU is at when a DMG boot ROM exits, fitted to GBMicrotest's
/// poweron tests, which read LY, STAT, OAM and VRAM a set number of cycles
/// after the boot ROM.
const DMG_POST_BOOT_FRAME_POSITION: (u8, u16) = (153, 396);
/// LY and dot the PPU is at when the CGB boot ROM exits into a CGB game,
/// fitted to Gambatte's display_startstate tests, which read LY and STAT a
/// set number of cycles after the boot ROM.
const CGB_POST_BOOT_FRAME_POSITION: (u8, u16) = (144, 164);
/// LY and dot the PPU is at when the DMG-0 boot ROM exits.
const DMG0_POST_BOOT_FRAME_POSITION: (u8, u16) = (145, 154);
/// How far channel 1, still playing the boot sound, lags behind where our boot
/// ROMs leave it, in 2 MHz ticks: the real boot ROMs play the sound at slightly
/// different times. Fitted to Gambatte's ch1_init_pos tests, which retrigger
/// the channel at different times after the boot ROM and hear which step of
/// its duty cycle it stopped on.
const DMG_POST_BOOT_SQUARE_1_DELAY_TICKS: i32 = 334;
const CGB_POST_BOOT_SQUARE_1_DELAY_TICKS: i32 = 86;
/// How many frame sequencer steps the APU is further on when the boot ROM
/// exits than ours leave it, since ours switch the APU on at another time.
/// Fitted to Gambatte's ch2_init_env_counter_timing tests, on DMG and CGB.
const POST_BOOT_FRAME_SEQUENCER_STEPS: u8 = 4;
/// The system counter when the CGB boot ROM exits into a CGB game, fitted to
/// Gambatte's div start_inc tests, which read DIV just after the boot ROM.
/// Mooneye's boot_div test only checks DMG games on the CGB
/// (`post_boot_system_counter`).
const CGB_POST_BOOT_SYSTEM_COUNTER: u16 = 0x1E9C;
/// The joypad select bits the SGB boot ROM leaves behind.
const SGB_POST_BOOT_JOYPAD_SELECT: u8 = 0x30;
/// The SGB system counter after the boot ROM, before the adjustment for the
/// cartridge header.
const SGB_POST_BOOT_SYSTEM_COUNTER_BASE: u16 = 0xDC84;
/// The part of the cartridge header the SGB boot ROM sends to the SNES.
const SGB_TRANSFERRED_HEADER: std::ops::Range<u16> = 0x0104..0x0150;
/// Where the cartridge header says whether the game supports the Super Game
/// Boy: 0x03 if it does.
const SGB_FLAG_ADDRESS: u16 = 0x0146;
/// Where the cartridge header has the old licensee code, which must be 0x33
/// for the Super Game Boy's commands to work.
const OLD_LICENSEE_CODE_ADDRESS: u16 = 0x014B;
/// How many ticks each set bit in the transferred header moves the SGB system
/// counter back.
const SGB_HEADER_BIT_COUNTER_TICKS: u32 = 4;

impl AddressBus {
    /// Loads the CGB or DMG boot ROM.
    pub(crate) fn load_boot_rom(&mut self, is_cgb: bool) {
        self.boot_rom = if is_cgb {
            CGB_BOOT.to_vec()
        } else {
            DMG_BOOT.to_vec()
        }
    }

    /// Returns true once after the boot ROM exits, so the CPU can load the
    /// registers the boot ROM leaves behind.
    pub(crate) fn take_boot_rom_exit(&mut self) -> bool {
        std::mem::take(&mut self.boot_rom_exited)
    }

    /// Unmaps the boot ROM and moves the timers and PPU to where they are on
    /// real hardware when it exits. These values are fitted per model to test
    /// ROMs (Mooneye's boot tests, GBMicrotest's poweron tests and Gambatte's
    /// display_startstate, ch1_init_pos, ch2_init_env_counter_timing and div
    /// start_inc tests; see each constant).
    pub(super) fn exit_boot_rom(&mut self) {
        self.catch_up_ppu_before_change();
        self.in_boot_rom = false;
        self.boot_rom_exited = true;

        if !self.cgb_mode {
            self.set_post_boot_system_counter(self.post_boot_system_counter());
            let (ly, dot) = if self.model == Model::Dmg0 {
                DMG0_POST_BOOT_FRAME_POSITION
            } else {
                DMG_POST_BOOT_FRAME_POSITION
            };
            self.gpu.set_frame_position(ly, dot);
        } else if self.gpu.in_dmg_compatibility_mode() {
            self.set_post_boot_system_counter(self.post_boot_system_counter());
        } else {
            self.set_post_boot_system_counter(CGB_POST_BOOT_SYSTEM_COUNTER);
            let (ly, dot) = CGB_POST_BOOT_FRAME_POSITION;
            self.gpu.set_frame_position(ly, dot);
        }

        self.apu.delay_boot_sound_phase(if self.cgb_mode {
            CGB_POST_BOOT_SQUARE_1_DELAY_TICKS
        } else {
            DMG_POST_BOOT_SQUARE_1_DELAY_TICKS
        });

        self.apu
            .advance_frame_sequencer(POST_BOOT_FRAME_SEQUENCER_STEPS);

        if self.model.is_sgb() {
            self.joypad.write_byte(SGB_POST_BOOT_JOYPAD_SELECT);
            self.apu.silence_boot_sound();
            let sgb_flag = self.cartridge.read_rom(SGB_FLAG_ADDRESS);
            let old_licensee_code = self.cartridge.read_rom(OLD_LICENSEE_CODE_ADDRESS);
            if let Some(sgb) = self.gpu.sgb_mut() {
                sgb.start(sgb_flag, old_licensee_code);
            }
        }
    }

    /// Moves the system counter to `value`, where the model's own boot ROM
    /// leaves it. The jump stands for time the real boot ROM spends
    /// differently rather than time passing, so the APU only learns the new
    /// value, without an edge of its bits (`exit_boot_rom` puts the frame
    /// sequencer in place itself). The serial port sees the falling edges
    /// between the old and the new value at its next step, as if the counter
    /// had counted there.
    fn set_post_boot_system_counter(&mut self, value: u16) {
        self.timers.set_system_counter(value);
        self.apu.system_counter_changed(value, value, false);
    }

    /// The system counter (the 16-bit counter whose upper byte is DIV) when
    /// the boot ROM exits.
    fn post_boot_system_counter(&self) -> u16 {
        match self.model {
            Model::Dmg0 => 0x182C,
            Model::Dmg | Model::Mgb => 0xABC8,
            Model::Sgb | Model::Sgb2 => self.sgb_post_boot_system_counter(),
            Model::Cgb0 => 0x2880,
            Model::CgbA | Model::CgbB | Model::CgbC | Model::CgbD | Model::CgbE => 0x2674,
            Model::Agb => 0x2678,
        }
    }

    /// The SGB system counter after the boot ROM depends on the cartridge:
    /// each set bit in the header it sends to the SNES moves it back.
    fn sgb_post_boot_system_counter(&self) -> u16 {
        let header_set_bits: u32 = SGB_TRANSFERRED_HEADER
            .map(|address| self.cartridge.read_rom(address).count_ones())
            .sum();
        SGB_POST_BOOT_SYSTEM_COUNTER_BASE
            .wrapping_sub((header_set_bits * SGB_HEADER_BIT_COUNTER_TICKS) as u16)
    }
}

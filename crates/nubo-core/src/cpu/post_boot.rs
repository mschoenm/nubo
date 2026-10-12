//! The register values each hardware model's boot ROM leaves behind.
//!
//! The boot ROM is the small program built into the console that shows the
//! logo and then hands over to the cartridge. The emulator ships one custom
//! boot ROM per family (Bootix for DMG models, a SameBoy-based one for CGB
//! models), so when it finishes, these values make the CPU look as if the
//! model's own boot ROM had run (checked by Mooneye's boot_regs tests).

use crate::cpu::{CARRY_FLAG, Cpu, HALF_CARRY_FLAG, ZERO_FLAG};
use crate::model::Model;

const POST_BOOT_STACK_POINTER: u16 = 0xFFFE;
/// Where the cartridge header keeps its checksum.
const HEADER_CHECKSUM_ADDRESS: u16 = 0x014D;

impl Cpu {
    /// Sets the registers the model's boot ROM leaves when it hands over to the
    /// cartridge. CGB models other than the AGB keep what the boot ROM left.
    pub(super) fn apply_post_boot_registers(&mut self) {
        let model = self.address_bus.model();
        if model == Model::Agb {
            self.increment_b_like_the_agb_boot_rom();
            return;
        }

        let Some([a, mut f, b, c, d, e, h, l]) = dmg_family_post_boot_registers(model) else {
            return;
        };
        if matches!(model, Model::Dmg | Model::Mgb) {
            let header_checksum = self
                .address_bus
                .cartridge()
                .read_rom(HEADER_CHECKSUM_ADDRESS);
            f = dmg_post_boot_flags(header_checksum);
        }
        let registers = &mut self.registers;
        registers.a = a;
        registers.f = f;
        registers.b = b;
        registers.c = c;
        registers.d = d;
        registers.e = e;
        registers.h = h;
        registers.l = l;
        registers.stack_pointer = POST_BOOT_STACK_POINTER;
    }

    /// The AGB (Game Boy Advance) runs the CGB boot ROM with one extra
    /// `INC B` at the end, which this repeats on top of the CGB values.
    fn increment_b_like_the_agb_boot_rom(&mut self) {
        let registers = &mut self.registers;
        let result = registers.b.wrapping_add(1);
        let mut flags = registers.f & CARRY_FLAG;
        if result == 0 {
            flags |= ZERO_FLAG;
        }
        if registers.b & 0x0F == 0x0F {
            flags |= HALF_CARRY_FLAG;
        }
        registers.b = result;
        registers.f = flags;
    }
}

/// The flags the DMG and MGB boot ROMs leave behind. The last instruction that
/// sets them is the `ADD A, [HL]` that adds the header checksum to the sum of
/// the other header bytes, where the result must be 0 for the boot ROM to go
/// on. So Z is set and N clear, and the addition carries (C) unless the
/// checksum is 0x00, and carries out of the low four bits (H) unless the
/// checksum's low four bits are 0. Pan Docs says H and C are both clear for a
/// checksum of 0x00 and both set otherwise, which also holds for every
/// checksum but those ending in 0, such as 0x10.
fn dmg_post_boot_flags(header_checksum: u8) -> u8 {
    let mut flags = ZERO_FLAG;
    if header_checksum != 0 {
        flags |= CARRY_FLAG;
    }
    if header_checksum & 0x0F != 0 {
        flags |= HALF_CARRY_FLAG;
    }
    flags
}

/// A, F, B, C, D, E, H and L after the boot ROM of a DMG, MGB or SGB model.
/// The DMG's and MGB's F depends on the cartridge (`dmg_post_boot_flags`).
fn dmg_family_post_boot_registers(model: Model) -> Option<[u8; 8]> {
    match model {
        Model::Dmg0 => Some([0x01, 0x00, 0xFF, 0x13, 0x00, 0xC1, 0x84, 0x03]),
        Model::Dmg => Some([0x01, 0xB0, 0x00, 0x13, 0x00, 0xD8, 0x01, 0x4D]),
        Model::Mgb => Some([0xFF, 0xB0, 0x00, 0x13, 0x00, 0xD8, 0x01, 0x4D]),
        Model::Sgb => Some([0x01, 0x00, 0x00, 0x14, 0x00, 0x00, 0xC0, 0x60]),
        Model::Sgb2 => Some([0xFF, 0x00, 0x00, 0x14, 0x00, 0x00, 0xC0, 0x60]),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dmg_flags_follow_the_addition_of_the_header_checksum() {
        assert_eq!(dmg_post_boot_flags(0x00), 0x80);
        assert_eq!(dmg_post_boot_flags(0x10), 0x90);
        assert_eq!(dmg_post_boot_flags(0x4D), 0xB0);
    }
}

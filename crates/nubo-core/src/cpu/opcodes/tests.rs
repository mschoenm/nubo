use crate::address_bus::AddressBus;
use crate::address_bus::constants::*;
use crate::address_bus::effects::empty_cartridge_effects;
use crate::address_bus::test_utils::build_rom;
use crate::cpu::{BusActivityEntry, BusActivityKind};
use crate::cpu::{Cpu, InterruptMasterEnable, RunState};
use crate::joypad::Key;
use crate::model::Model;
use crate::serializable::Serializable;

fn init_rom_with_test_instructions(test_instructions: &[u8]) -> Vec<u8> {
    let mut rom = build_rom(CARTRIDGE_TYPE_ROM_ONLY, ROM_SIZE_64KB, RAM_SIZE_2KB);
    rom[..test_instructions.len()].copy_from_slice(test_instructions);
    rom
}

fn init_cpu_from_rom(rom: Vec<u8>) -> Cpu {
    let address_bus = AddressBus::new(false);
    let mut cpu = Cpu::new(address_bus);
    cpu.address_bus
        .load_rom(rom, empty_cartridge_effects())
        .unwrap();
    cpu.address_bus.set_in_boot_rom(false);

    // The Game Boy actually uses a decode/execute/prefetch loop, where fetching
    // the next instruction is the last step. Initially, the first instruction is always a NOP.
    // Source: https://gist.github.com/SonoSooS/c0055300670d678b5ae8433e20bea595#fetch-and-stuff
    // This is why we need to step twice to get to the first opcode under test.
    cpu.step();

    cpu
}

/// Reads `address` as the memory map holds it, with the PPU caught up first.
fn read(cpu: &mut Cpu, address: u16) -> u8 {
    cpu.address_bus.catch_up_ppu();
    cpu.address_bus.peek(address)
}

/// Writes `address` in the memory map, with the PPU caught up first.
fn write(cpu: &mut Cpu, address: u16, value: u8) {
    cpu.address_bus.catch_up_ppu();
    cpu.address_bus.poke(address, value);
}

/// Runs one `Cpu::step` and returns how many PPU dots (four per M-cycle at
/// normal speed) it took.
fn step_counting_clock_cycles(cpu: &mut Cpu) -> u64 {
    cpu.address_bus.run_deferred_cycles();
    let start = cpu.address_bus.elapsed_dots();
    cpu.step();
    cpu.address_bus.run_deferred_cycles();
    cpu.address_bus.elapsed_dots() - start
}

fn init_cpu_with_test_instructions(test_instructions: &[u8]) -> Cpu {
    let rom = init_rom_with_test_instructions(test_instructions);
    init_cpu_from_rom(rom)
}

#[test]
fn loads_immediate_byte_into_register_b() {
    let mut cpu = init_cpu_with_test_instructions(&[0x06, 0xA1]);
    let clock_cycles = step_counting_clock_cycles(&mut cpu);
    assert_eq!(cpu.registers.b, 0xA1);
    assert_eq!(cpu.registers.program_counter, 3);
    assert_eq!(clock_cycles, 8);
}

#[test]
fn loads_register_b_into_register_a() {
    let mut cpu = init_cpu_with_test_instructions(&[0x78]);
    cpu.registers.b = 0x2F;
    let clock_cycles = step_counting_clock_cycles(&mut cpu);
    assert_eq!(cpu.registers.a, 0x2F);
    assert_eq!(cpu.registers.program_counter, 2);
    assert_eq!(clock_cycles, 4);
}

#[test]
fn loads_byte_at_address_hl_into_register_a() {
    let mut rom = init_rom_with_test_instructions(&[0x7e]);
    rom[0x5550] = 0xB1;
    let mut cpu = init_cpu_from_rom(rom);
    cpu.registers.h = 0x55;
    cpu.registers.l = 0x50;
    let clock_cycles = step_counting_clock_cycles(&mut cpu);
    assert_eq!(cpu.registers.a, 0xB1);
    assert_eq!(cpu.registers.program_counter, 2);
    assert_eq!(clock_cycles, 8);
}

#[test]
fn loads_register_b_into_address_hl() {
    let mut cpu = init_cpu_with_test_instructions(&[0x70]);
    cpu.registers.b = 0x5A;
    cpu.registers.h = 0x81;
    cpu.registers.l = 0x9B;
    let clock_cycles = step_counting_clock_cycles(&mut cpu);
    assert_eq!(read(&mut cpu, 0x819B), 0x5A);
    assert_eq!(clock_cycles, 8);
}

#[test]
fn loads_immediate_byte_into_memory() {
    let mut cpu = init_cpu_with_test_instructions(&[0x36, 0xE6]);
    cpu.registers.h = 0x82;
    cpu.registers.l = 0x44;
    let clock_cycles = step_counting_clock_cycles(&mut cpu);
    assert_eq!(read(&mut cpu, 0x8244), 0xE6);
    assert_eq!(clock_cycles, 12);
}

#[test]
fn loads_byte_at_address_nn_into_register_a() {
    let mut rom = init_rom_with_test_instructions(&[0xFA, 0x1C, 0x4B]);
    rom[0x4B1C] = 0x22;
    let mut cpu = init_cpu_from_rom(rom);
    let clock_cycles = step_counting_clock_cycles(&mut cpu);
    assert_eq!(cpu.registers.a, 0x22);
    assert_eq!(clock_cycles, 16);
}

#[test]
fn loads_byte_at_ff00_plus_register_c_into_register_a() {
    let mut cpu = init_cpu_with_test_instructions(&[0xF2]);
    write(&mut cpu, 0xFF9B, 0x9A);
    cpu.registers.c = 0x9B;
    let clock_cycles = step_counting_clock_cycles(&mut cpu);
    assert_eq!(cpu.registers.a, 0x9A);
    assert_eq!(clock_cycles, 8);
}

#[test]
fn loads_register_a_into_ff00_plus_register_c() {
    let mut cpu = init_cpu_with_test_instructions(&[0xE2]);
    cpu.registers.a = 0x9A;
    cpu.registers.c = 0x9B;
    let clock_cycles = step_counting_clock_cycles(&mut cpu);
    assert_eq!(read(&mut cpu, 0xFF9B), 0x9A);
    assert_eq!(clock_cycles, 8);
}

#[test]
fn loads_byte_at_address_hl_into_register_a_then_decrements_hl() {
    let mut rom = init_rom_with_test_instructions(&[0x3a]);
    rom[0x2AB1] = 0xAA;
    let mut cpu = init_cpu_from_rom(rom);
    cpu.registers.h = 0x2A;
    cpu.registers.l = 0xB1;
    let clock_cycles = step_counting_clock_cycles(&mut cpu);
    assert_eq!(cpu.registers.a, 0xAA);
    assert_eq!(cpu.registers.program_counter, 2);
    assert_eq!(cpu.registers.h, 0x2A);
    assert_eq!(cpu.registers.l, 0xB0);
    assert_eq!(clock_cycles, 8);
}

#[test]
fn loads_register_a_into_address_hl_then_decrements_hl() {
    let mut cpu = init_cpu_with_test_instructions(&[0x32]);
    cpu.registers.a = 0xBB;
    cpu.registers.h = 0x8A;
    cpu.registers.l = 0xB1;
    let clock_cycles = step_counting_clock_cycles(&mut cpu);
    assert_eq!(cpu.registers.program_counter, 2);
    assert_eq!(read(&mut cpu, 0x8AB1), 0xBB);
    assert_eq!(cpu.registers.h, 0x8A);
    assert_eq!(cpu.registers.l, 0xB0);
    assert_eq!(clock_cycles, 8);
}

#[test]
fn loads_byte_at_address_hl_into_register_a_then_increments_hl() {
    let mut rom = init_rom_with_test_instructions(&[0x2A]);
    rom[0x2AB1] = 0xAA;
    let mut cpu = init_cpu_from_rom(rom);
    cpu.registers.h = 0x2A;
    cpu.registers.l = 0xB1;
    let clock_cycles = step_counting_clock_cycles(&mut cpu);
    assert_eq!(cpu.registers.a, 0xAA);
    assert_eq!(cpu.registers.program_counter, 2);
    assert_eq!(cpu.registers.h, 0x2A);
    assert_eq!(cpu.registers.l, 0xB2);
    assert_eq!(clock_cycles, 8);
}

#[test]
fn loads_register_a_into_address_hl_then_increments_hl() {
    let mut cpu = init_cpu_with_test_instructions(&[0x22]);
    cpu.registers.a = 0xBB;
    cpu.registers.h = 0x8A;
    cpu.registers.l = 0xB1;
    let clock_cycles = step_counting_clock_cycles(&mut cpu);
    assert_eq!(cpu.registers.program_counter, 2);
    assert_eq!(read(&mut cpu, 0x8AB1), 0xBB);
    assert_eq!(cpu.registers.h, 0x8A);
    assert_eq!(cpu.registers.l, 0xB2);
    assert_eq!(clock_cycles, 8);
}

#[test]
fn loads_register_a_into_ff00_plus_immediate_byte() {
    let mut cpu = init_cpu_with_test_instructions(&[0xE0, 0xB1]);
    cpu.registers.a = 0x9A;
    let clock_cycles = step_counting_clock_cycles(&mut cpu);
    assert_eq!(read(&mut cpu, 0xFFB1), 0x9A);
    assert_eq!(clock_cycles, 12);
}

#[test]
fn loads_byte_at_address_ff00_plus_immediate_byte_into_register_a() {
    let mut cpu = init_cpu_with_test_instructions(&[0xF0, 0xB1]);
    write(&mut cpu, 0xFFB1, 0x9A);
    let clock_cycles = step_counting_clock_cycles(&mut cpu);
    assert_eq!(cpu.registers.a, 0x9A);
    assert_eq!(clock_cycles, 12);
}

#[test]
fn loads_immediate_word_into_register_pair_bc() {
    let mut cpu = init_cpu_with_test_instructions(&[0x01, 0xA2, 0xA3]);
    let clock_cycles = step_counting_clock_cycles(&mut cpu);
    assert_eq!(cpu.registers.b, 0xA3);
    assert_eq!(cpu.registers.c, 0xA2);
    assert_eq!(clock_cycles, 12);
}

#[test]
fn loads_immediate_word_into_stack_pointer() {
    let mut cpu = init_cpu_with_test_instructions(&[0x31, 0xA2, 0xA3]);
    let clock_cycles = step_counting_clock_cycles(&mut cpu);
    assert_eq!(cpu.registers.stack_pointer, 0xA3A2);
    assert_eq!(clock_cycles, 12);
}

#[test]
fn loads_word_at_register_pair_hl_into_stack_pointer() {
    let mut cpu = init_cpu_with_test_instructions(&[0xF9]);
    cpu.registers.h = 0xAB;
    cpu.registers.l = 0x13;
    let clock_cycles = step_counting_clock_cycles(&mut cpu);
    assert_eq!(cpu.registers.stack_pointer, 0xAB13);
    assert_eq!(clock_cycles, 8);
}

#[test]
fn loads_stack_pointer_plus_immediate_byte_into_register_pair_hl_with_half_carry() {
    let mut cpu = init_cpu_with_test_instructions(&[0xF8, 0x19]);
    cpu.registers.stack_pointer = 0xB207;
    let clock_cycles = step_counting_clock_cycles(&mut cpu);
    assert_eq!(cpu.registers.h, 0xB2);
    assert_eq!(cpu.registers.l, 0x20);
    assert_eq!(cpu.registers.stack_pointer, 0xB207);
    assert_eq!(cpu.registers.f, 0x20);
    assert_eq!(clock_cycles, 12);
}

#[test]
fn loads_stack_pointer_plus_immediate_byte_into_register_pair_hl_with_carry() {
    let mut cpu = init_cpu_with_test_instructions(&[0xF8, 0x19]);
    cpu.registers.stack_pointer = 0xB2F7;
    let clock_cycles = step_counting_clock_cycles(&mut cpu);
    assert_eq!(cpu.registers.h, 0xB3);
    assert_eq!(cpu.registers.l, 0x10);
    assert_eq!(cpu.registers.stack_pointer, 0xB2F7);
    assert_eq!(cpu.registers.f, 0x30);
    assert_eq!(clock_cycles, 12);
}

#[test]
fn loads_stack_pointer_into_address_nn() {
    let mut cpu = init_cpu_with_test_instructions(&[0x08, 0x13, 0x82]);
    cpu.registers.stack_pointer = 0x9BB2;
    let clock_cycles = step_counting_clock_cycles(&mut cpu);
    assert_eq!(read(&mut cpu, 0x8213), 0xB2);
    assert_eq!(read(&mut cpu, 0x8214), 0x9B);
    assert_eq!(clock_cycles, 20);
}

#[test]
fn pushes_register_pair_onto_stack() {
    let mut cpu = init_cpu_with_test_instructions(&[0xC5]);
    cpu.registers.b = 0xB1;
    cpu.registers.c = 0xDD;
    cpu.registers.stack_pointer = 0xFFFE;
    let clock_cycles = step_counting_clock_cycles(&mut cpu);
    assert_eq!(read(&mut cpu, 0xFFFD), 0xB1);
    assert_eq!(read(&mut cpu, 0xFFFC), 0xDD);
    assert_eq!(cpu.registers.stack_pointer, 0xFFFC);
    assert_eq!(clock_cycles, 16);
}

#[test]
fn pops_word_into_register_pair_from_stack() {
    let mut rom = init_rom_with_test_instructions(&[0xC1]);
    rom[0x2111] = 0xB1;
    rom[0x2110] = 0xDD;
    let mut cpu = init_cpu_from_rom(rom);
    cpu.registers.stack_pointer = 0x2110;
    let clock_cycles = step_counting_clock_cycles(&mut cpu);
    assert_eq!(cpu.registers.b, 0xB1);
    assert_eq!(cpu.registers.c, 0xDD);
    assert_eq!(cpu.registers.stack_pointer, 0x2112);
    assert_eq!(clock_cycles, 12);
}

#[test]
fn adds_register_and_register_a_with_half_carry() {
    let mut cpu = init_cpu_with_test_instructions(&[0x80]);
    cpu.registers.a = 0x2B;
    cpu.registers.b = 0xAF;
    let clock_cycles = step_counting_clock_cycles(&mut cpu);
    assert_eq!(cpu.registers.a, 0xDA);
    assert_eq!(cpu.registers.b, 0xAF);
    assert_eq!(cpu.registers.f, 0x20);
    assert_eq!(clock_cycles, 4);
}

#[test]
fn adds_register_and_register_a_with_carry() {
    let mut cpu = init_cpu_with_test_instructions(&[0x80]);
    cpu.registers.a = 0xC1;
    cpu.registers.b = 0x5A;
    let clock_cycles = step_counting_clock_cycles(&mut cpu);
    assert_eq!(cpu.registers.a, 0x1B);
    assert_eq!(cpu.registers.b, 0x5A);
    assert_eq!(cpu.registers.f, 0x10);
    assert_eq!(clock_cycles, 4);
}

#[test]
fn adds_register_and_register_a_and_carry_flag() {
    let mut cpu = init_cpu_with_test_instructions(&[0x88]);
    cpu.registers.a = 0x2B;
    cpu.registers.b = 0xBE;
    cpu.registers.f = 0x10;
    let clock_cycles = step_counting_clock_cycles(&mut cpu);
    assert_eq!(cpu.registers.a, 0xEA);
    assert_eq!(cpu.registers.b, 0xBE);
    assert_eq!(cpu.registers.f, 0x20);
    assert_eq!(clock_cycles, 4);
}

#[test]
fn subtracts_register_value_from_register_a_with_half_carry() {
    let mut cpu = init_cpu_with_test_instructions(&[0x90]);
    cpu.registers.a = 0xB1;
    cpu.registers.b = 0x7F;
    let clock_cycles = step_counting_clock_cycles(&mut cpu);
    assert_eq!(cpu.registers.a, 0x32);
    assert_eq!(cpu.registers.b, 0x7F);
    assert_eq!(cpu.registers.f, 0x60);
    assert_eq!(clock_cycles, 4);
}

#[test]
fn subtracts_register_value_from_register_a_with_carry() {
    let mut cpu = init_cpu_with_test_instructions(&[0x90]);
    cpu.registers.a = 0x02;
    cpu.registers.b = 0x04;
    let clock_cycles = step_counting_clock_cycles(&mut cpu);
    assert_eq!(cpu.registers.a, 0xFE);
    assert_eq!(cpu.registers.b, 0x04);
    assert_eq!(cpu.registers.f, 0x70);
    assert_eq!(clock_cycles, 4);
}

#[test]
fn subtracts_register_value_plus_carry_from_register_a_with_half_carry() {
    let mut cpu = init_cpu_with_test_instructions(&[0x98]);
    cpu.registers.a = 0xB1;
    cpu.registers.b = 0x74;
    cpu.registers.f = 0x10;
    let clock_cycles = step_counting_clock_cycles(&mut cpu);
    assert_eq!(cpu.registers.a, 0x3C);
    assert_eq!(cpu.registers.b, 0x74);
    assert_eq!(cpu.registers.f, 0x60);
    assert_eq!(clock_cycles, 4);
}

#[test]
fn subtracts_register_value_plus_carry_from_register_a_with_carry() {
    let mut cpu = init_cpu_with_test_instructions(&[0x98]);
    cpu.registers.a = 0x02;
    cpu.registers.b = 0x04;
    cpu.registers.f = 0x10;
    let clock_cycles = step_counting_clock_cycles(&mut cpu);
    assert_eq!(cpu.registers.a, 0xFD);
    assert_eq!(cpu.registers.b, 0x04);
    assert_eq!(cpu.registers.f, 0x70);
    assert_eq!(clock_cycles, 4);
}

#[test]
fn logical_ands_register_and_register_a() {
    let mut cpu = init_cpu_with_test_instructions(&[0xA0]);
    cpu.registers.a = 0x15;
    cpu.registers.b = 0x7E;
    let clock_cycles = step_counting_clock_cycles(&mut cpu);
    assert_eq!(cpu.registers.a, 0x14);
    assert_eq!(cpu.registers.b, 0x7E);
    assert_eq!(cpu.registers.f, 0x20);
    assert_eq!(clock_cycles, 4);
}

#[test]
fn logical_ors_register_and_register_a() {
    let mut cpu = init_cpu_with_test_instructions(&[0xB0]);
    cpu.registers.a = 0x15;
    cpu.registers.b = 0x7E;
    let clock_cycles = step_counting_clock_cycles(&mut cpu);
    assert_eq!(cpu.registers.a, 0x7F);
    assert_eq!(cpu.registers.b, 0x7E);
    assert_eq!(cpu.registers.f, 0x0);
    assert_eq!(clock_cycles, 4);
}

#[test]
fn logical_xors_register_and_register_a() {
    let mut cpu = init_cpu_with_test_instructions(&[0xA8]);
    cpu.registers.a = 0x15;
    cpu.registers.b = 0x7E;
    let clock_cycles = step_counting_clock_cycles(&mut cpu);
    assert_eq!(cpu.registers.a, 0x6B);
    assert_eq!(cpu.registers.b, 0x7E);
    assert_eq!(cpu.registers.f, 0x0);
    assert_eq!(clock_cycles, 4);
}

const ILLEGAL_OPCODES: [u8; 11] = [
    0xD3, 0xDB, 0xDD, 0xE3, 0xE4, 0xEB, 0xEC, 0xED, 0xF4, 0xFC, 0xFD,
];

#[test]
fn locks_up_on_illegal_opcode() {
    for opcode in ILLEGAL_OPCODES {
        let mut cpu = init_cpu_with_test_instructions(&[opcode, 0x3C]);
        cpu.registers.stack_pointer = 0xC100;
        cpu.ime = InterruptMasterEnable::On;
        write(&mut cpu, 0xFFFF, 0x1F);
        write(&mut cpu, 0xFF0F, 0x01);
        let system_counter = cpu.address_bus.system_counter();

        for _ in 0..10 {
            let clock_cycles = step_counting_clock_cycles(&mut cpu);
            assert_eq!(clock_cycles, 4);
        }

        assert_eq!(cpu.registers.opcode, opcode);
        assert_eq!(cpu.registers.program_counter, 1);
        assert_eq!(cpu.registers.stack_pointer, 0xC100);
        assert_eq!(cpu.registers.a, 0);
        cpu.address_bus.run_deferred_cycles();
        assert_eq!(
            cpu.address_bus.system_counter(),
            system_counter.wrapping_add(40)
        );
    }
}

#[test]
fn stays_locked_up_after_loading_a_save_state() {
    let mut cpu = init_cpu_with_test_instructions(&[0xFC, 0x3C]);
    cpu.step();
    cpu.address_bus.run_deferred_cycles();
    let mut save_state = Vec::new();
    cpu.serialize(&mut save_state).unwrap();

    let mut restored_cpu = init_cpu_with_test_instructions(&[0x00, 0x3C]);
    restored_cpu
        .deserialize(&mut save_state.as_slice())
        .unwrap();
    restored_cpu.step();
    restored_cpu.step();

    assert_eq!(restored_cpu.registers.program_counter, 1);
    assert_eq!(restored_cpu.registers.a, 0);
}

#[test]
fn wraps_program_counter_when_running_code_at_ffff() {
    let mut cpu = init_cpu_with_test_instructions(&[0xC3, 0xFF, 0xFF]);
    write(&mut cpu, 0xFFFF, 0x00);
    cpu.step();
    assert_eq!(cpu.registers.program_counter, 0x0000);
    assert_eq!(cpu.registers.opcode, 0x00);

    cpu.step();
    assert_eq!(cpu.registers.program_counter, 0x0001);
    assert_eq!(cpu.registers.opcode, 0xC3);
}

#[test]
fn wraps_address_when_storing_stack_pointer_at_ffff() {
    let mut cpu = init_cpu_with_test_instructions(&[0x08, 0xFF, 0xFF]);
    cpu.registers.stack_pointer = 0x1234;
    cpu.step();
    assert_eq!(cpu.address_bus.interrupt_enable(), 0x34);
}

#[test]
fn compares_register_value_with_register_a_resulting_in_half_carry() {
    let mut cpu = init_cpu_with_test_instructions(&[0xB8]);
    cpu.registers.a = 0xB1;
    cpu.registers.b = 0x7F;
    let clock_cycles = step_counting_clock_cycles(&mut cpu);
    assert_eq!(cpu.registers.a, 0xB1);
    assert_eq!(cpu.registers.b, 0x7F);
    assert_eq!(cpu.registers.f, 0x60);
    assert_eq!(clock_cycles, 4);
}

#[test]
fn compares_register_value_with_register_a_resulting_in_carry() {
    let mut cpu = init_cpu_with_test_instructions(&[0xB8]);
    cpu.registers.a = 0x02;
    cpu.registers.b = 0x04;
    let clock_cycles = step_counting_clock_cycles(&mut cpu);
    assert_eq!(cpu.registers.a, 0x02);
    assert_eq!(cpu.registers.b, 0x04);
    assert_eq!(cpu.registers.f, 0x70);
    assert_eq!(clock_cycles, 4);
}

#[test]
fn increments_register_with_half_carry() {
    let mut cpu = init_cpu_with_test_instructions(&[0x04]);
    cpu.registers.b = 0x0F;
    let clock_cycles = step_counting_clock_cycles(&mut cpu);
    assert_eq!(cpu.registers.b, 0x10);
    assert_eq!(cpu.registers.f, 0x20);
    assert_eq!(clock_cycles, 4);
}

#[test]
fn increments_register_without_carry() {
    let mut cpu = init_cpu_with_test_instructions(&[0x04]);
    cpu.registers.b = 0xA3;
    let clock_cycles = step_counting_clock_cycles(&mut cpu);
    assert_eq!(cpu.registers.b, 0xA4);
    assert_eq!(cpu.registers.f, 0x00);
    assert_eq!(clock_cycles, 4);
}

#[test]
fn decrements_register_with_half_carry() {
    let mut cpu = init_cpu_with_test_instructions(&[0x05]);
    cpu.registers.b = 0x10;
    let clock_cycles = step_counting_clock_cycles(&mut cpu);
    assert_eq!(cpu.registers.b, 0x0F);
    assert_eq!(cpu.registers.f, 0x60);
    assert_eq!(clock_cycles, 4);
}

#[test]
fn decrements_register_without_carry() {
    let mut cpu = init_cpu_with_test_instructions(&[0x05]);
    cpu.registers.b = 0xA3;
    let clock_cycles = step_counting_clock_cycles(&mut cpu);
    assert_eq!(cpu.registers.b, 0xA2);
    assert_eq!(cpu.registers.f, 0x40);
    assert_eq!(clock_cycles, 4);
}

#[test]
fn increments_memory_byte_with_half_carry() {
    let mut cpu = init_cpu_with_test_instructions(&[0x34]);
    write(&mut cpu, 0x8C11, 0x0F);
    cpu.registers.h = 0x8C;
    cpu.registers.l = 0x11;
    let clock_cycles = step_counting_clock_cycles(&mut cpu);
    assert_eq!(read(&mut cpu, 0x8C11), 0x10);
    assert_eq!(cpu.registers.f, 0x20);
    assert_eq!(clock_cycles, 12);
}

#[test]
fn decrements_memory_byte_with_half_carry() {
    let mut cpu = init_cpu_with_test_instructions(&[0x35]);
    write(&mut cpu, 0x8C11, 0x10);
    cpu.registers.h = 0x8C;
    cpu.registers.l = 0x11;
    let clock_cycles = step_counting_clock_cycles(&mut cpu);
    assert_eq!(read(&mut cpu, 0x8C11), 0x0F);
    assert_eq!(cpu.registers.f, 0x60);
    assert_eq!(clock_cycles, 12);
}

#[test]
fn increments_register_pair() {
    let mut cpu = init_cpu_with_test_instructions(&[0x03]);
    cpu.registers.b = 0x3C;
    cpu.registers.c = 0x4D;
    let clock_cycles = step_counting_clock_cycles(&mut cpu);
    assert_eq!(cpu.registers.b, 0x3C);
    assert_eq!(cpu.registers.c, 0x4E);
    assert_eq!(clock_cycles, 8);
}

#[test]
fn decrements_register_pair() {
    let mut cpu = init_cpu_with_test_instructions(&[0x0B]);
    cpu.registers.b = 0x3C;
    cpu.registers.c = 0x4D;
    let clock_cycles = step_counting_clock_cycles(&mut cpu);
    assert_eq!(cpu.registers.b, 0x3C);
    assert_eq!(cpu.registers.c, 0x4C);
    assert_eq!(clock_cycles, 8);
}

#[test]
fn increments_stack_pointer() {
    let mut cpu = init_cpu_with_test_instructions(&[0x33]);
    cpu.registers.stack_pointer = 0x1A33;
    let clock_cycles = step_counting_clock_cycles(&mut cpu);
    assert_eq!(cpu.registers.stack_pointer, 0x1A34);
    assert_eq!(clock_cycles, 8);
}

#[test]
fn decrements_stack_pointer() {
    let mut cpu = init_cpu_with_test_instructions(&[0x3B]);
    cpu.registers.stack_pointer = 0x1A33;
    let clock_cycles = step_counting_clock_cycles(&mut cpu);
    assert_eq!(cpu.registers.stack_pointer, 0x1A32);
    assert_eq!(clock_cycles, 8);
}

#[test]
fn adds_register_pair_and_register_pair_hl_with_carry() {
    let mut cpu = init_cpu_with_test_instructions(&[0x09]);
    cpu.registers.h = 0xFF;
    cpu.registers.l = 0xFE;
    cpu.registers.b = 0x00;
    cpu.registers.c = 0x04;
    let clock_cycles = step_counting_clock_cycles(&mut cpu);
    assert_eq!(cpu.registers.h, 0x00);
    assert_eq!(cpu.registers.l, 0x02);
    assert_eq!(cpu.registers.b, 0x00);
    assert_eq!(cpu.registers.c, 0x04);
    assert_eq!(cpu.registers.f, 0x30);
    assert_eq!(clock_cycles, 8);
}

#[test]
fn adds_register_pair_and_register_pair_hl_with_half_carry() {
    let mut cpu = init_cpu_with_test_instructions(&[0x09]);
    cpu.registers.h = 0xDF;
    cpu.registers.l = 0xFF;
    cpu.registers.b = 0x00;
    cpu.registers.c = 0x01;
    let clock_cycles = step_counting_clock_cycles(&mut cpu);
    assert_eq!(cpu.registers.h, 0xE0);
    assert_eq!(cpu.registers.l, 0x00);
    assert_eq!(cpu.registers.b, 0x00);
    assert_eq!(cpu.registers.c, 0x01);
    assert_eq!(cpu.registers.f, 0x20);
    assert_eq!(clock_cycles, 8);
}

#[test]
fn loads_stack_pointer_plus_immediate_byte_into_stack_pointer_with_half_carry() {
    let mut cpu = init_cpu_with_test_instructions(&[0xE8, 0x19]);
    cpu.registers.stack_pointer = 0xB207;
    let clock_cycles = step_counting_clock_cycles(&mut cpu);
    assert_eq!(cpu.registers.stack_pointer, 0xB220);
    assert_eq!(cpu.registers.f, 0x20);
    assert_eq!(clock_cycles, 16);
}

#[test]
fn loads_stack_pointer_plus_immediate_byte_into_stack_pointer_with_carry() {
    let mut cpu = init_cpu_with_test_instructions(&[0xE8, 0x19]);
    cpu.registers.stack_pointer = 0xB2F7;
    let clock_cycles = step_counting_clock_cycles(&mut cpu);
    assert_eq!(cpu.registers.stack_pointer, 0xB310);
    assert_eq!(cpu.registers.f, 0x30);
    assert_eq!(clock_cycles, 16);
}

#[test]
fn swaps_nibbles_in_register() {
    let mut cpu = init_cpu_with_test_instructions(&[0xCB, 0x37]);
    cpu.registers.a = 0xA2;
    let clock_cycles = step_counting_clock_cycles(&mut cpu);
    assert_eq!(cpu.registers.a, 0x2A);
    assert_eq!(clock_cycles, 8);
}

#[test]
fn swaps_nibbles_in_memory_byte() {
    let mut cpu = init_cpu_with_test_instructions(&[0xCB, 0x36]);
    write(&mut cpu, 0x8AB1, 0xBC);
    cpu.registers.h = 0x8A;
    cpu.registers.l = 0xB1;
    let clock_cycles = step_counting_clock_cycles(&mut cpu);
    assert_eq!(read(&mut cpu, 0x8AB1), 0xCB);
    assert_eq!(clock_cycles, 16);
}

#[test]
fn sets_carry_flag() {
    let mut cpu = init_cpu_with_test_instructions(&[0x37]);
    let clock_cycles = step_counting_clock_cycles(&mut cpu);
    assert_eq!(cpu.registers.f, 0x10);
    assert_eq!(clock_cycles, 4);
}

#[test]
fn complement_a_register() {
    let mut cpu = init_cpu_with_test_instructions(&[0x2F]);
    cpu.registers.a = 0x4C;
    let clock_cycles = step_counting_clock_cycles(&mut cpu);
    assert_eq!(cpu.registers.a, 0xB3);
    assert_eq!(cpu.registers.f, 0x60);
    assert_eq!(clock_cycles, 4);
}

#[test]
fn complement_c_flag() {
    let mut cpu = init_cpu_with_test_instructions(&[0x3F]);
    cpu.registers.f = 0x30;
    let clock_cycles = step_counting_clock_cycles(&mut cpu);
    assert_eq!(cpu.registers.f, 0x00);
    assert_eq!(clock_cycles, 4);
}

#[test]
fn decimal_adjusts_register_a() {
    let mut cpu = init_cpu_with_test_instructions(&[0x27]);
    cpu.registers.a = 0xC0;
    let clock_cycles = step_counting_clock_cycles(&mut cpu);
    assert_eq!(cpu.registers.a, 0x20);
    assert_eq!(cpu.registers.f, 0x10);
    assert_eq!(clock_cycles, 4);
}

#[test]
fn rotates_register_a_left() {
    let mut cpu = init_cpu_with_test_instructions(&[0x07]);
    cpu.registers.a = 0xA7;
    let clock_cycles = step_counting_clock_cycles(&mut cpu);
    assert_eq!(cpu.registers.a, 0x4F);
    assert_eq!(cpu.registers.f, 0x10);
    assert_eq!(clock_cycles, 4);
}

#[test]
fn rotates_register_a_left_through_carry() {
    let mut cpu = init_cpu_with_test_instructions(&[0x17]);
    cpu.registers.a = 0xA7;
    let clock_cycles = step_counting_clock_cycles(&mut cpu);
    assert_eq!(cpu.registers.a, 0x4E);
    assert_eq!(cpu.registers.f, 0x10);
    assert_eq!(clock_cycles, 4);
}

#[test]
fn rotates_register_a_left_and_resets_z_flag_even_if_result_is_zero() {
    let mut cpu = init_cpu_with_test_instructions(&[0x17]);
    cpu.registers.a = 0x0;
    let clock_cycles = step_counting_clock_cycles(&mut cpu);
    assert_eq!(cpu.registers.a, 0x0);
    assert_eq!(cpu.registers.f, 0x0);
    assert_eq!(clock_cycles, 4);
}

#[test]
fn rotates_register_a_right() {
    let mut cpu = init_cpu_with_test_instructions(&[0x0F]);
    cpu.registers.a = 0xA7;
    let clock_cycles = step_counting_clock_cycles(&mut cpu);
    assert_eq!(cpu.registers.a, 0xD3);
    assert_eq!(cpu.registers.f, 0x10);
    assert_eq!(clock_cycles, 4);
}

#[test]
fn rotates_register_a_right_through_carry() {
    let mut cpu = init_cpu_with_test_instructions(&[0x1F]);
    cpu.registers.a = 0xA7;
    let clock_cycles = step_counting_clock_cycles(&mut cpu);
    assert_eq!(cpu.registers.a, 0x53);
    assert_eq!(cpu.registers.f, 0x10);
    assert_eq!(clock_cycles, 4);
}

#[test]
fn rotates_memory_location_hl_left() {
    let mut cpu = init_cpu_with_test_instructions(&[0xCB, 0x06]);
    cpu.registers.h = 0x93;
    cpu.registers.l = 0xDA;
    write(&mut cpu, 0x93DA, 0xA7);
    let clock_cycles = step_counting_clock_cycles(&mut cpu);
    assert_eq!(read(&mut cpu, 0x93DA), 0x4F);
    assert_eq!(cpu.registers.f, 0x10);
    assert_eq!(clock_cycles, 16);
}

#[test]
fn rotates_memory_location_hl_left_through_carry() {
    let mut cpu = init_cpu_with_test_instructions(&[0xCB, 0x16]);
    cpu.registers.h = 0x9A;
    cpu.registers.l = 0x51;
    write(&mut cpu, 0x9A51, 0xA7);
    let clock_cycles = step_counting_clock_cycles(&mut cpu);
    assert_eq!(read(&mut cpu, 0x9A51), 0x4E);
    assert_eq!(cpu.registers.f, 0x10);
    assert_eq!(clock_cycles, 16);
}

#[test]
fn rotates_memory_location_hl_right() {
    let mut cpu = init_cpu_with_test_instructions(&[0xCB, 0x0E]);
    cpu.registers.h = 0x9A;
    cpu.registers.l = 0xAC;
    write(&mut cpu, 0x9AAC, 0xA7);
    let clock_cycles = step_counting_clock_cycles(&mut cpu);
    assert_eq!(read(&mut cpu, 0x9AAC), 0xD3);
    assert_eq!(cpu.registers.f, 0x10);
    assert_eq!(clock_cycles, 16);
}

#[test]
fn rotates_memory_location_hl_right_through_carry() {
    let mut cpu = init_cpu_with_test_instructions(&[0xCB, 0x1E]);
    cpu.registers.h = 0x9A;
    cpu.registers.l = 0x51;
    write(&mut cpu, 0x9A51, 0xA7);
    let clock_cycles = step_counting_clock_cycles(&mut cpu);
    assert_eq!(read(&mut cpu, 0x9A51), 0x53);
    assert_eq!(cpu.registers.f, 0x10);
    assert_eq!(clock_cycles, 16);
}

#[test]
fn shifts_register_a_left() {
    let mut cpu = init_cpu_with_test_instructions(&[0xCB, 0x27]);
    cpu.registers.a = 0xA7;
    let clock_cycles = step_counting_clock_cycles(&mut cpu);
    assert_eq!(cpu.registers.a, 0x4E);
    assert_eq!(cpu.registers.f, 0x10);
    assert_eq!(clock_cycles, 8);
}

#[test]
fn shifts_memory_location_hl_left() {
    let mut cpu = init_cpu_with_test_instructions(&[0xCB, 0x26]);
    cpu.registers.h = 0x9A;
    cpu.registers.l = 0x51;
    write(&mut cpu, 0x9A51, 0xA7);
    let clock_cycles = step_counting_clock_cycles(&mut cpu);
    assert_eq!(read(&mut cpu, 0x9A51), 0x4E);
    assert_eq!(cpu.registers.f, 0x10);
    assert_eq!(clock_cycles, 16);
}

#[test]
fn shifts_register_a_right_maintaining_msb() {
    let mut cpu = init_cpu_with_test_instructions(&[0xCB, 0x2F]);
    cpu.registers.a = 0xA7;
    let clock_cycles = step_counting_clock_cycles(&mut cpu);
    assert_eq!(cpu.registers.a, 0xD3);
    assert_eq!(cpu.registers.f, 0x10);
    assert_eq!(clock_cycles, 8);
}

#[test]
fn shifts_memory_location_hl_right_maintaining_msb() {
    let mut cpu = init_cpu_with_test_instructions(&[0xCB, 0x2E]);
    cpu.registers.h = 0x9A;
    cpu.registers.l = 0x51;
    write(&mut cpu, 0x9A51, 0xA7);
    let clock_cycles = step_counting_clock_cycles(&mut cpu);
    assert_eq!(read(&mut cpu, 0x9A51), 0xD3);
    assert_eq!(cpu.registers.f, 0x10);
    assert_eq!(clock_cycles, 16);
}

#[test]
fn shifts_register_a_right() {
    let mut cpu = init_cpu_with_test_instructions(&[0xCB, 0x3F]);
    cpu.registers.a = 0xA7;
    let clock_cycles = step_counting_clock_cycles(&mut cpu);
    assert_eq!(cpu.registers.a, 0x53);
    assert_eq!(cpu.registers.f, 0x10);
    assert_eq!(clock_cycles, 8);
}

#[test]
fn shifts_memory_location_hl_right() {
    let mut cpu = init_cpu_with_test_instructions(&[0xCB, 0x3E]);
    cpu.registers.h = 0x9A;
    cpu.registers.l = 0x51;
    write(&mut cpu, 0x9A51, 0xA7);
    let clock_cycles = step_counting_clock_cycles(&mut cpu);
    assert_eq!(read(&mut cpu, 0x9A51), 0x53);
    assert_eq!(cpu.registers.f, 0x10);
    assert_eq!(clock_cycles, 16);
}

#[test]
fn test_bit_0_of_register_a() {
    let mut cpu = init_cpu_with_test_instructions(&[0xCB, 0x47]);
    cpu.registers.a = 0xB5;
    let clock_cycles = step_counting_clock_cycles(&mut cpu);
    assert_eq!(cpu.registers.f, 0x20);
    assert_eq!(clock_cycles, 8);
}

#[test]
fn test_bit_1_of_register_a() {
    let mut cpu = init_cpu_with_test_instructions(&[0xCB, 0x4F]);
    cpu.registers.a = 0xB5;
    let clock_cycles = step_counting_clock_cycles(&mut cpu);
    assert_eq!(cpu.registers.f, 0xA0);
    assert_eq!(clock_cycles, 8);
}

#[test]
fn test_bit_2_of_register_a() {
    let mut cpu = init_cpu_with_test_instructions(&[0xCB, 0x57]);
    cpu.registers.a = 0xB5;
    let clock_cycles = step_counting_clock_cycles(&mut cpu);
    assert_eq!(cpu.registers.f, 0x20);
    assert_eq!(clock_cycles, 8);
}

#[test]
fn test_bit_3_of_register_a() {
    let mut cpu = init_cpu_with_test_instructions(&[0xCB, 0x5F]);
    cpu.registers.a = 0xB5;
    let clock_cycles = step_counting_clock_cycles(&mut cpu);
    assert_eq!(cpu.registers.f, 0xA0);
    assert_eq!(clock_cycles, 8);
}

#[test]
fn test_bit_4_of_register_a() {
    let mut cpu = init_cpu_with_test_instructions(&[0xCB, 0x67]);
    cpu.registers.a = 0xB5;
    let clock_cycles = step_counting_clock_cycles(&mut cpu);
    assert_eq!(cpu.registers.f, 0x20);
    assert_eq!(clock_cycles, 8);
}

#[test]
fn test_bit_5_of_register_a() {
    let mut cpu = init_cpu_with_test_instructions(&[0xCB, 0x6F]);
    cpu.registers.a = 0xB5;
    let clock_cycles = step_counting_clock_cycles(&mut cpu);
    assert_eq!(cpu.registers.f, 0x20);
    assert_eq!(clock_cycles, 8);
}

#[test]
fn test_bit_6_of_register_a() {
    let mut cpu = init_cpu_with_test_instructions(&[0xCB, 0x77]);
    cpu.registers.a = 0xB5;
    let clock_cycles = step_counting_clock_cycles(&mut cpu);
    assert_eq!(cpu.registers.f, 0xA0);
    assert_eq!(clock_cycles, 8);
}

#[test]
fn test_bit_7_of_register_a() {
    let mut cpu = init_cpu_with_test_instructions(&[0xCB, 0x7F]);
    cpu.registers.a = 0xB5;
    let clock_cycles = step_counting_clock_cycles(&mut cpu);
    assert_eq!(cpu.registers.f, 0x20);
    assert_eq!(clock_cycles, 8);
}

#[test]
fn reset_bit_0_of_register_a() {
    let mut cpu = init_cpu_with_test_instructions(&[0xCB, 0x87]);
    cpu.registers.a = 0xB5;
    let clock_cycles = step_counting_clock_cycles(&mut cpu);
    assert_eq!(cpu.registers.a, 0xB4);
    assert_eq!(clock_cycles, 8);
}

#[test]
fn set_bit_0_of_register_a() {
    let mut cpu = init_cpu_with_test_instructions(&[0xCB, 0xC7]);
    cpu.registers.a = 0xB5;
    let clock_cycles = step_counting_clock_cycles(&mut cpu);
    assert_eq!(cpu.registers.a, 0xB5);
    assert_eq!(clock_cycles, 8);
}

#[test]
fn reset_bit_1_of_register_a() {
    let mut cpu = init_cpu_with_test_instructions(&[0xCB, 0x8F]);
    cpu.registers.a = 0xB5;
    let clock_cycles = step_counting_clock_cycles(&mut cpu);
    assert_eq!(cpu.registers.a, 0xB5);
    assert_eq!(clock_cycles, 8);
}

#[test]
fn set_bit_1_of_register_a() {
    let mut cpu = init_cpu_with_test_instructions(&[0xCB, 0xCF]);
    cpu.registers.a = 0xB5;
    let clock_cycles = step_counting_clock_cycles(&mut cpu);
    assert_eq!(cpu.registers.a, 0xB7);
    assert_eq!(clock_cycles, 8);
}

#[test]
fn reset_bit_2_of_register_a() {
    let mut cpu = init_cpu_with_test_instructions(&[0xCB, 0x97]);
    cpu.registers.a = 0xB5;
    let clock_cycles = step_counting_clock_cycles(&mut cpu);
    assert_eq!(cpu.registers.a, 0xB1);
    assert_eq!(clock_cycles, 8);
}

#[test]
fn set_bit_2_of_register_a() {
    let mut cpu = init_cpu_with_test_instructions(&[0xCB, 0xD7]);
    cpu.registers.a = 0xB5;
    let clock_cycles = step_counting_clock_cycles(&mut cpu);
    assert_eq!(cpu.registers.a, 0xB5);
    assert_eq!(clock_cycles, 8);
}

#[test]
fn reset_bit_3_of_register_a() {
    let mut cpu = init_cpu_with_test_instructions(&[0xCB, 0x9F]);
    cpu.registers.a = 0xB5;
    let clock_cycles = step_counting_clock_cycles(&mut cpu);
    assert_eq!(cpu.registers.a, 0xB5);
    assert_eq!(clock_cycles, 8);
}

#[test]
fn set_bit_3_of_register_a() {
    let mut cpu = init_cpu_with_test_instructions(&[0xCB, 0xDF]);
    cpu.registers.a = 0xB5;
    let clock_cycles = step_counting_clock_cycles(&mut cpu);
    assert_eq!(cpu.registers.a, 0xBD);
    assert_eq!(clock_cycles, 8);
}

#[test]
fn reset_bit_4_of_register_a() {
    let mut cpu = init_cpu_with_test_instructions(&[0xCB, 0xA7]);
    cpu.registers.a = 0xB5;
    let clock_cycles = step_counting_clock_cycles(&mut cpu);
    assert_eq!(cpu.registers.a, 0xA5);
    assert_eq!(clock_cycles, 8);
}

#[test]
fn set_bit_4_of_register_a() {
    let mut cpu = init_cpu_with_test_instructions(&[0xCB, 0xE7]);
    cpu.registers.a = 0xB5;
    let clock_cycles = step_counting_clock_cycles(&mut cpu);
    assert_eq!(cpu.registers.a, 0xB5);
    assert_eq!(clock_cycles, 8);
}

#[test]
fn reset_bit_5_of_register_a() {
    let mut cpu = init_cpu_with_test_instructions(&[0xCB, 0xAF]);
    cpu.registers.a = 0xB5;
    let clock_cycles = step_counting_clock_cycles(&mut cpu);
    assert_eq!(cpu.registers.a, 0x95);
    assert_eq!(clock_cycles, 8);
}

#[test]
fn set_bit_5_of_register_a() {
    let mut cpu = init_cpu_with_test_instructions(&[0xCB, 0xEF]);
    cpu.registers.a = 0xB5;
    let clock_cycles = step_counting_clock_cycles(&mut cpu);
    assert_eq!(cpu.registers.a, 0xB5);
    assert_eq!(clock_cycles, 8);
}

#[test]
fn reset_bit_6_of_register_a() {
    let mut cpu = init_cpu_with_test_instructions(&[0xCB, 0xB7]);
    cpu.registers.a = 0xB5;
    let clock_cycles = step_counting_clock_cycles(&mut cpu);
    assert_eq!(cpu.registers.a, 0xB5);
    assert_eq!(clock_cycles, 8);
}

#[test]
fn set_bit_6_of_register_a() {
    let mut cpu = init_cpu_with_test_instructions(&[0xCB, 0xF7]);
    cpu.registers.a = 0xB5;
    let clock_cycles = step_counting_clock_cycles(&mut cpu);
    assert_eq!(cpu.registers.a, 0xF5);
    assert_eq!(clock_cycles, 8);
}

#[test]
fn reset_bit_7_of_register_a() {
    let mut cpu = init_cpu_with_test_instructions(&[0xCB, 0xBF]);
    cpu.registers.a = 0xB5;
    let clock_cycles = step_counting_clock_cycles(&mut cpu);
    assert_eq!(cpu.registers.a, 0x35);
    assert_eq!(clock_cycles, 8);
}

#[test]
fn set_bit_7_of_register_a() {
    let mut cpu = init_cpu_with_test_instructions(&[0xCB, 0xFF]);
    cpu.registers.a = 0xB5;
    let clock_cycles = step_counting_clock_cycles(&mut cpu);
    assert_eq!(cpu.registers.a, 0xB5);
    assert_eq!(clock_cycles, 8);
}

#[test]
fn jumps_to_address_nn() {
    let mut cpu = init_cpu_with_test_instructions(&[0xC3, 0xAA, 0x54]);
    let clock_cycles = step_counting_clock_cycles(&mut cpu);
    assert_eq!(cpu.registers.program_counter, 0x54AB);
    assert_eq!(clock_cycles, 16);
}

#[test]
fn avoids_jumping_to_address_nn_if_z_flag_is_reset() {
    let mut cpu = init_cpu_with_test_instructions(&[0xC2, 0xAA, 0x54]);
    cpu.registers.f = 0x80;
    let clock_cycles = step_counting_clock_cycles(&mut cpu);
    assert_eq!(cpu.registers.program_counter, 0x04);
    assert_eq!(clock_cycles, 12);
}

#[test]
fn jumps_to_address_nn_if_z_flag_is_set() {
    let mut cpu = init_cpu_with_test_instructions(&[0xCA, 0xAA, 0x54]);
    cpu.registers.f = 0x80;
    let clock_cycles = step_counting_clock_cycles(&mut cpu);
    assert_eq!(cpu.registers.program_counter, 0x54AB);
    assert_eq!(clock_cycles, 16);
}

#[test]
fn jumps_to_address_nn_if_c_flag_is_reset() {
    let mut cpu = init_cpu_with_test_instructions(&[0xD2, 0xAA, 0x54]);
    cpu.registers.f = 0x80;
    let clock_cycles = step_counting_clock_cycles(&mut cpu);
    assert_eq!(cpu.registers.program_counter, 0x54AB);
    assert_eq!(clock_cycles, 16);
}

#[test]
fn jumps_to_address_nn_if_c_flag_is_set() {
    let mut cpu = init_cpu_with_test_instructions(&[0xDA, 0xAA, 0x54]);
    cpu.registers.f = 0x80;
    let clock_cycles = step_counting_clock_cycles(&mut cpu);
    assert_eq!(cpu.registers.program_counter, 0x04);
    assert_eq!(clock_cycles, 12);
}

#[test]
fn jumps_to_address_hl() {
    let mut cpu = init_cpu_with_test_instructions(&[0xE9]);
    cpu.registers.h = 0x4B;
    cpu.registers.l = 0x51;
    let clock_cycles = step_counting_clock_cycles(&mut cpu);
    assert_eq!(cpu.registers.program_counter, 0x4B52);
    assert_eq!(clock_cycles, 4);
}

#[test]
fn jumps_to_current_address_plus_n() {
    let mut cpu = init_cpu_with_test_instructions(&[0x18, 0x05]);
    let clock_cycles = step_counting_clock_cycles(&mut cpu);
    assert_eq!(cpu.registers.program_counter, 0x08);
    assert_eq!(clock_cycles, 12);
}

#[test]
fn ignores_jumping_to_curent_address_plus_n_if_z_flag_is_set() {
    let mut cpu = init_cpu_with_test_instructions(&[0x20, 0x05]);
    cpu.registers.f = 0x80;
    let clock_cycles = step_counting_clock_cycles(&mut cpu);
    assert_eq!(cpu.registers.program_counter, 0x03);
    assert_eq!(clock_cycles, 8);
}

#[test]
fn jumps_to_current_address_plus_n_if_z_flag_is_reset() {
    let mut cpu = init_cpu_with_test_instructions(&[0x20, 0x02]);
    let clock_cycles = step_counting_clock_cycles(&mut cpu);
    assert_eq!(cpu.registers.program_counter, 0x05);
    assert_eq!(clock_cycles, 12);
}

#[test]
fn jumps_to_current_address_minus_n_if_z_flag_is_reset() {
    let mut cpu = init_cpu_with_test_instructions(&[0x20, 0xFE]);
    let clock_cycles = step_counting_clock_cycles(&mut cpu);
    assert_eq!(cpu.registers.program_counter, 0x01);
    assert_eq!(clock_cycles, 12);
}

#[test]
fn jumps_to_current_address_plus_n_if_z_flag_is_set() {
    let mut cpu = init_cpu_with_test_instructions(&[0x28, 0x05]);
    cpu.registers.f = 0x80;
    let clock_cycles = step_counting_clock_cycles(&mut cpu);
    assert_eq!(cpu.registers.program_counter, 0x08);
    assert_eq!(clock_cycles, 12);
}

#[test]
fn jumps_to_current_address_plus_n_if_c_flag_is_reset() {
    let mut cpu = init_cpu_with_test_instructions(&[0x30, 0x05]);
    cpu.registers.f = 0x80;
    let clock_cycles = step_counting_clock_cycles(&mut cpu);
    assert_eq!(cpu.registers.program_counter, 0x08);
    assert_eq!(clock_cycles, 12);
}

#[test]
fn jumps_to_curent_address_plus_n_if_c_flag_is_set() {
    let mut cpu = init_cpu_with_test_instructions(&[0x38, 0x05]);
    cpu.registers.f = 0x80;
    let clock_cycles = step_counting_clock_cycles(&mut cpu);
    assert_eq!(cpu.registers.program_counter, 0x03);
    assert_eq!(clock_cycles, 8);
}

#[test]
fn calls_address_nn() {
    let mut cpu = init_cpu_with_test_instructions(&[0xCD, 0x4A, 0x51]);
    cpu.registers.stack_pointer = 0xFFFE;
    let clock_cycles = step_counting_clock_cycles(&mut cpu);
    assert_eq!(read(&mut cpu, 0xFFFD), 0x00);
    assert_eq!(read(&mut cpu, 0xFFFC), 0x03);
    assert_eq!(cpu.registers.stack_pointer, 0xFFFC);
    assert_eq!(cpu.registers.program_counter, 0x514B);
    assert_eq!(clock_cycles, 24);
}

#[test]
fn calls_address_nn_if_z_flag_is_reset() {
    let mut cpu = init_cpu_with_test_instructions(&[0xC4, 0x4A, 0x51]);
    cpu.registers.stack_pointer = 0xFFFE;
    cpu.registers.f = 0x80;
    let clock_cycles = step_counting_clock_cycles(&mut cpu);
    assert_eq!(read(&mut cpu, 0xFFFD), 0x00);
    assert_eq!(read(&mut cpu, 0xFFFC), 0x00);
    assert_eq!(cpu.registers.stack_pointer, 0xFFFE);
    assert_eq!(cpu.registers.program_counter, 0x04);
    assert_eq!(clock_cycles, 12);
}

#[test]
fn calls_address_nn_if_z_flag_is_set() {
    let mut cpu = init_cpu_with_test_instructions(&[0xCC, 0x4A, 0x51]);
    cpu.registers.stack_pointer = 0xFFFE;
    cpu.registers.f = 0x80;
    let clock_cycles = step_counting_clock_cycles(&mut cpu);
    assert_eq!(read(&mut cpu, 0xFFFD), 0x00);
    assert_eq!(read(&mut cpu, 0xFFFC), 0x03);
    assert_eq!(cpu.registers.stack_pointer, 0xFFFC);
    assert_eq!(cpu.registers.program_counter, 0x514B);
    assert_eq!(clock_cycles, 24);
}

#[test]
fn calls_address_nn_if_c_flag_is_reset() {
    let mut cpu = init_cpu_with_test_instructions(&[0xD4, 0x4A, 0x51]);
    cpu.registers.stack_pointer = 0xFFFE;
    cpu.registers.f = 0x80;
    let clock_cycles = step_counting_clock_cycles(&mut cpu);
    assert_eq!(read(&mut cpu, 0xFFFD), 0x00);
    assert_eq!(read(&mut cpu, 0xFFFC), 0x03);
    assert_eq!(cpu.registers.stack_pointer, 0xFFFC);
    assert_eq!(cpu.registers.program_counter, 0x514B);
    assert_eq!(clock_cycles, 24);
}

#[test]
fn calls_address_nn_if_c_flag_is_set() {
    let mut cpu = init_cpu_with_test_instructions(&[0xDC, 0x4A, 0x51]);
    cpu.registers.stack_pointer = 0x2112;
    cpu.registers.f = 0x80;
    let clock_cycles = step_counting_clock_cycles(&mut cpu);
    assert_eq!(cpu.registers.stack_pointer, 0x2112);
    assert_eq!(cpu.registers.program_counter, 0x04);
    assert_eq!(clock_cycles, 12);
}

#[test]
fn restarts_address_0() {
    let mut cpu = init_cpu_with_test_instructions(&[0x00, 0x00, 0xC7]);
    cpu.registers.stack_pointer = 0xFFFE;
    cpu.step();
    cpu.step();
    cpu.step();
    assert_eq!(read(&mut cpu, 0xFFFD), 0x00);
    assert_eq!(read(&mut cpu, 0xFFFC), 0x03);
    assert_eq!(cpu.registers.stack_pointer, 0xFFFC);
    assert_eq!(cpu.registers.program_counter, 0x01);
}

#[test]
fn returns_from_call() {
    let mut rom = init_rom_with_test_instructions(&[0xC9]);
    rom[0x2111] = 0xB1;
    rom[0x2110] = 0xDD;
    let mut cpu = init_cpu_from_rom(rom);
    cpu.registers.stack_pointer = 0x2110;
    let clock_cycles = step_counting_clock_cycles(&mut cpu);
    assert_eq!(cpu.registers.program_counter, 0xB1DE);
    assert_eq!(cpu.registers.stack_pointer, 0x2112);
    assert_eq!(clock_cycles, 16);
}

#[test]
fn returns_from_call_if_z_flag_is_reset() {
    let mut rom = init_rom_with_test_instructions(&[0xC0]);
    rom[0x2111] = 0xB1;
    rom[0x2110] = 0xDD;
    let mut cpu = init_cpu_from_rom(rom);
    cpu.registers.stack_pointer = 0x2110;
    cpu.registers.f = 0x80;
    let clock_cycles = step_counting_clock_cycles(&mut cpu);
    assert_eq!(cpu.registers.program_counter, 0x02);
    assert_eq!(cpu.registers.stack_pointer, 0x2110);
    assert_eq!(clock_cycles, 8);
}

#[test]
fn returns_from_call_if_z_flag_is_set() {
    let mut rom = init_rom_with_test_instructions(&[0xC8]);
    rom[0x2111] = 0xB1;
    rom[0x2110] = 0xDD;
    let mut cpu = init_cpu_from_rom(rom);
    cpu.registers.stack_pointer = 0x2110;
    cpu.registers.f = 0x80;
    let clock_cycles = step_counting_clock_cycles(&mut cpu);
    assert_eq!(cpu.registers.program_counter, 0xB1DE);
    assert_eq!(cpu.registers.stack_pointer, 0x2112);
    assert_eq!(clock_cycles, 20);
}

#[test]
fn returns_from_call_if_c_flag_is_reset() {
    let mut rom = init_rom_with_test_instructions(&[0xD0]);
    rom[0x2111] = 0xB1;
    rom[0x2110] = 0xDD;
    let mut cpu = init_cpu_from_rom(rom);
    cpu.registers.stack_pointer = 0x2110;
    cpu.registers.f = 0x80;
    let clock_cycles = step_counting_clock_cycles(&mut cpu);
    assert_eq!(cpu.registers.program_counter, 0xB1DE);
    assert_eq!(cpu.registers.stack_pointer, 0x2112);
    assert_eq!(clock_cycles, 20);
}

#[test]
fn returns_from_call_if_c_flag_is_set() {
    let mut rom = init_rom_with_test_instructions(&[0xD8]);
    rom[0x2111] = 0xB1;
    rom[0x2110] = 0xDD;
    let mut cpu = init_cpu_from_rom(rom);
    cpu.registers.stack_pointer = 0x2110;
    cpu.registers.f = 0x80;
    let clock_cycles = step_counting_clock_cycles(&mut cpu);
    assert_eq!(cpu.registers.program_counter, 0x02);
    assert_eq!(cpu.registers.stack_pointer, 0x2110);
    assert_eq!(clock_cycles, 8);
}

#[test]
fn halts_the_cpu_until_interrupt() {
    let mut cpu = init_cpu_with_test_instructions(&[0x76, 0x15]);
    cpu.step();
    assert!(cpu.is_halted());
    assert_eq!(cpu.registers.program_counter, 0x1);
    cpu.step();
    assert!(cpu.is_halted());
    assert_eq!(cpu.registers.program_counter, 0x1);
    cpu.step();
    assert!(cpu.is_halted());
    assert_eq!(cpu.registers.program_counter, 0x1);

    cpu.ime = InterruptMasterEnable::On;
    cpu.registers.stack_pointer = 0x2112;
    write(&mut cpu, 0xFFFF, 0x1F);
    write(&mut cpu, 0xFF0F, 0x01);

    cpu.step();
    assert!(!cpu.is_halted());
    assert_eq!(cpu.registers.program_counter, 0x41);
}

#[test]
fn returns_to_halt_after_an_interrupt_that_was_pending_when_it_ran() {
    let mut rom = init_rom_with_test_instructions(&[0xFB, 0x76, 0x00]);
    rom[0x40] = 0xD9;
    let mut cpu = init_cpu_from_rom(rom);
    cpu.registers.stack_pointer = 0xC100;
    write(&mut cpu, 0xFFFF, 0x01);
    write(&mut cpu, 0xFF0F, 0x01);

    // EI, then HALT, which finds the interrupt pending with IME on and hands
    // over to the VBlank handler, pushing the address of HALT itself.
    cpu.step();
    cpu.step();
    assert!(!cpu.is_halted());
    assert_eq!(cpu.registers.program_counter, 0x41);
    assert_eq!(cpu.registers.stack_pointer, 0xC0FE);
    assert_eq!(cpu.address_bus.cpu_read_byte(0xC0FE), 0x01);
    assert_eq!(cpu.address_bus.cpu_read_byte(0xC0FF), 0x00);

    // RETI returns to HALT, which runs again and now halts.
    cpu.step();
    assert_eq!(cpu.registers.program_counter, 0x02);
    cpu.step();
    assert!(cpu.is_halted());
}

#[test]
fn enables_interrupts() {
    let mut cpu = init_cpu_with_test_instructions(&[0xFB, 0x00]);
    cpu.step();
    assert_eq!(cpu.ime, InterruptMasterEnable::OnAfterNextInstruction);
    cpu.step();
    assert_eq!(cpu.ime, InterruptMasterEnable::On);
}

#[test]
fn leaves_ime_off_in_a_handler_entered_right_after_ei_with_ime_already_on() {
    let mut cpu = init_cpu_with_test_instructions(&[0xFB, 0x00]);
    cpu.ime = InterruptMasterEnable::On;
    cpu.registers.stack_pointer = 0xC100;
    write(&mut cpu, 0xFFFF, 0x01);
    write(&mut cpu, 0xFF0F, 0x01);

    cpu.step();
    assert_eq!(cpu.registers.program_counter, 0x41);
    assert_eq!(cpu.ime, InterruptMasterEnable::Off);
    cpu.step();
    assert_eq!(cpu.ime, InterruptMasterEnable::Off);
}

#[test]
fn disables_interrupts() {
    let mut cpu = init_cpu_with_test_instructions(&[0xF3]);
    cpu.ime = InterruptMasterEnable::On;
    cpu.step();
    assert_eq!(cpu.ime, InterruptMasterEnable::Off);
}

#[test]
fn returns_from_call_then_enables_interrupts() {
    let mut rom = init_rom_with_test_instructions(&[0xD9]);
    rom[0x2111] = 0xB1;
    rom[0x2110] = 0xDD;
    let mut cpu = init_cpu_from_rom(rom);
    cpu.registers.stack_pointer = 0x2110;
    let clock_cycles = step_counting_clock_cycles(&mut cpu);
    assert_eq!(cpu.registers.program_counter, 0xB1DE);
    assert_eq!(cpu.registers.stack_pointer, 0x2112);
    assert_eq!(cpu.ime, InterruptMasterEnable::On);
    assert_eq!(clock_cycles, 16);
}

#[test]
fn runs_vertical_blank_isr() {
    let mut cpu = init_cpu_with_test_instructions(&[0x00]);
    cpu.registers.stack_pointer = 0x2112;
    cpu.ime = InterruptMasterEnable::On;
    write(&mut cpu, 0xFFFF, 0x1F);
    write(&mut cpu, 0xFF0F, 0x01);
    cpu.step();
    assert_eq!(cpu.registers.stack_pointer, 0x2110);
    assert_eq!(cpu.ime, InterruptMasterEnable::Off);
    assert_eq!(cpu.registers.program_counter, 0x41);
    assert_eq!(cpu.address_bus.interrupt_enable(), 0x1F);
    assert_eq!(read(&mut cpu, 0xFF0F) & 0x01, 0);
}

#[test]
fn runs_lcd_status_isr() {
    let mut cpu = init_cpu_with_test_instructions(&[0x00]);
    cpu.registers.stack_pointer = 0x2112;
    cpu.ime = InterruptMasterEnable::On;
    write(&mut cpu, 0xFFFF, 0x1F);
    write(&mut cpu, 0xFF0F, 0x02);
    cpu.step();
    assert_eq!(cpu.registers.stack_pointer, 0x2110);
    assert_eq!(cpu.ime, InterruptMasterEnable::Off);
    assert_eq!(cpu.registers.program_counter, 0x49);
    assert_eq!(cpu.address_bus.interrupt_enable(), 0x1F);
    assert_eq!(read(&mut cpu, 0xFF0F) & 0x02, 0);
}

#[test]
fn runs_timer_overflow_isr() {
    let mut cpu = init_cpu_with_test_instructions(&[0x00]);
    cpu.registers.stack_pointer = 0x2112;
    cpu.ime = InterruptMasterEnable::On;
    write(&mut cpu, 0xFFFF, 0x1F);
    write(&mut cpu, 0xFF0F, 0x04);
    cpu.step();
    assert_eq!(cpu.registers.stack_pointer, 0x2110);
    assert_eq!(cpu.ime, InterruptMasterEnable::Off);
    assert_eq!(cpu.registers.program_counter, 0x51);
    assert_eq!(cpu.address_bus.interrupt_enable(), 0x1F);
    assert_eq!(read(&mut cpu, 0xFF0F) & 0x04, 0);
}

#[test]
fn runs_serial_link_isr() {
    let mut cpu = init_cpu_with_test_instructions(&[0x00]);
    cpu.registers.stack_pointer = 0x2112;
    cpu.ime = InterruptMasterEnable::On;
    write(&mut cpu, 0xFFFF, 0x1F);
    write(&mut cpu, 0xFF0F, 0x08);
    cpu.step();
    assert_eq!(cpu.registers.stack_pointer, 0x2110);
    assert_eq!(cpu.ime, InterruptMasterEnable::Off);
    assert_eq!(cpu.registers.program_counter, 0x59);
    assert_eq!(cpu.address_bus.interrupt_enable(), 0x1F);
    assert_eq!(read(&mut cpu, 0xFF0F) & 0x08, 0);
}

#[test]
fn runs_joypad_press_isr() {
    let mut cpu = init_cpu_with_test_instructions(&[0x00]);
    cpu.registers.stack_pointer = 0x2112;
    cpu.ime = InterruptMasterEnable::On;
    write(&mut cpu, 0xFFFF, 0x1F);
    write(&mut cpu, 0xFF0F, 0x10);
    cpu.step();
    assert_eq!(cpu.registers.stack_pointer, 0x2110);
    assert_eq!(cpu.ime, InterruptMasterEnable::Off);
    assert_eq!(cpu.registers.program_counter, 0x61);
    assert_eq!(cpu.address_bus.interrupt_enable(), 0x1F);
    assert_eq!(read(&mut cpu, 0xFF0F) & 0x10, 0);
}

#[test]
fn toggles_cgb_double_speed_mode() {
    let mut cpu = init_cpu_with_test_instructions(&[0x10]);
    cpu.address_bus.set_model(Model::CgbE);
    cpu.address_bus.load_boot_rom(true);
    write(&mut cpu, 0xFF4D, 0x01);
    cpu.step();
    assert_eq!(read(&mut cpu, 0xFF4D) & 0x01, 0);
    assert!(cpu.is_halted());
    // Double speed starts six units (one and a half M-cycles) after STOP.
    cpu.step();
    assert!(!cpu.address_bus.cgb_double_speed());
    cpu.step();
    assert!(cpu.address_bus.cgb_double_speed());
}

#[test]
fn stops_the_cpu_until_a_button_is_pressed() {
    let mut cpu = init_cpu_with_test_instructions(&[0x10, 0x00, 0x04]);
    cpu.step();
    assert_eq!(cpu.run_state, RunState::Stopped);
    assert_eq!(cpu.registers.program_counter, 0x2);
    for _ in 0..100 {
        cpu.step();
    }
    assert_eq!(cpu.run_state, RunState::Stopped);
    assert_eq!(cpu.address_bus.system_counter(), 0);

    cpu.address_bus.press_key(Key::A);
    cpu.step();
    assert_ne!(cpu.run_state, RunState::Stopped);
    assert_eq!(cpu.registers.program_counter, 0x3);
    cpu.step();
    assert_eq!(cpu.registers.b, 0x1);
}

#[test]
fn halts_instead_of_stopping_while_a_button_is_held() {
    let mut cpu = init_cpu_with_test_instructions(&[0x10, 0x00, 0x04]);
    cpu.address_bus.press_key(Key::A);
    cpu.step();
    assert_ne!(cpu.run_state, RunState::Stopped);
    assert!(cpu.is_halted());
    assert_eq!(cpu.registers.program_counter, 0x2);
}

#[test]
fn records_bus_activity_for_loading_immediate_byte_into_register_b() {
    let address_bus = AddressBus::new(true);
    let mut cpu = Cpu::new(address_bus);

    cpu.address_bus.processor_test_ram_mut()[0x00] = 0x06;
    cpu.address_bus.processor_test_ram_mut()[0x01] = 0xA1;

    // Step once to make sure the first opcode is loaded
    cpu.step();

    // Step again to execute opcode 0x06
    cpu.step();

    let bus_activity = cpu.opcode_bus_activity;
    assert_eq!(bus_activity.len(), 2);

    let immediate_byte_read = BusActivityEntry {
        address: 0x01,
        value: 0xA1,
        kind: BusActivityKind::Read,
    };
    assert_eq!(bus_activity[0], Some(immediate_byte_read));

    let opcode_read = BusActivityEntry {
        address: 0x02,
        value: 0x00,
        kind: BusActivityKind::Read,
    };
    assert_eq!(bus_activity[1], Some(opcode_read));
}

#[test]
fn records_bus_activity_for_jump_to_address_nn() {
    let address_bus = AddressBus::new(true);
    let mut cpu = Cpu::new(address_bus);

    cpu.address_bus.processor_test_ram_mut()[0x00] = 0xC3;
    cpu.address_bus.processor_test_ram_mut()[0x01] = 0xAA;
    cpu.address_bus.processor_test_ram_mut()[0x02] = 0x54;

    // Step once to make sure the first opcode is loaded
    cpu.step();

    // Step again to execute opcode 0xC3
    cpu.step();

    let bus_activity = cpu.opcode_bus_activity;
    assert_eq!(bus_activity.len(), 4);

    let first_byte_read = BusActivityEntry {
        address: 0x01,
        value: 0xAA,
        kind: BusActivityKind::Read,
    };
    assert_eq!(bus_activity[0], Some(first_byte_read));

    let second_byte_read = BusActivityEntry {
        address: 0x02,
        value: 0x54,
        kind: BusActivityKind::Read,
    };
    assert_eq!(bus_activity[1], Some(second_byte_read));

    // Machine cycle with no bus activity
    assert_eq!(bus_activity[2], None);

    let opcode_read = BusActivityEntry {
        address: 0x54AA,
        value: 0x0,
        kind: BusActivityKind::Read,
    };
    assert_eq!(bus_activity[3], Some(opcode_read));
}

//! Tests of the memory map: which component each address reaches.

use crate::address_bus::constants::*;
use crate::address_bus::effects::empty_cartridge_effects;
use crate::address_bus::test_utils::*;
use crate::model::Model;

use super::*;

fn setup_test_address_bus() -> AddressBus {
    let mut address_bus = initialize_test_address_bus();

    address_bus.boot_rom[0] = 0xAF;
    address_bus.boot_rom[1] = 0xF1;
    address_bus.boot_rom[2] = 0x03;
    address_bus.boot_rom[3] = 0x4D;

    let mut rom = build_rom(CARTRIDGE_TYPE_MBC1, ROM_SIZE_64KB, RAM_SIZE_2KB);

    rom[0] = 0x1E;
    rom[1] = 0xF2;
    rom[2] = 0x01;
    rom[3] = 0x09;

    rom[0x20AF] = 0x11;
    rom[0x20B0] = 0x17;
    rom[0x20B1] = 0xEE;

    rom[0x5ACC] = 0x13;
    rom[0x5ACD] = 0x9C;
    rom[0x5ACE] = 0x55;

    address_bus
        .load_rom(rom, empty_cartridge_effects())
        .unwrap();

    let mut ram = vec![0; 0x800];
    ram[0] = 0xC2;
    ram[1] = 0x22;
    ram[2] = 0x35;
    address_bus.set_cartridge_ram(ram);

    address_bus.gpu.set_video_ram_byte(0, 0xB1);
    address_bus.gpu.set_video_ram_byte(1, 0xD2);
    address_bus.gpu.set_video_ram_byte(2, 0xAA);

    address_bus.working_ram[0] = 0xF1;
    address_bus.working_ram[1] = 0x22;
    address_bus.working_ram[2] = 0x2B;

    address_bus.working_ram[0x1001] = 0x11;

    address_bus.working_ram[0x15F0] = 0x2B;
    address_bus.working_ram[0x15F1] = 0x7C;

    address_bus.working_ram[0x2001] = 0x22;

    address_bus.gpu.set_object_attribute_memory_byte(0x7A, 0x44);
    address_bus.gpu.set_object_attribute_memory_byte(0x7B, 0x45);
    address_bus.gpu.set_object_attribute_memory_byte(0x7C, 0x9B);

    address_bus.high_ram[0x20] = 0xBB;
    address_bus.high_ram[0x21] = 0x44;
    address_bus.high_ram[0x5B] = 0x5F;

    address_bus.interrupt_enable = 0x1F;

    for _ in 0..0x3A * 64 {
        address_bus.timers.step();
    }
    address_bus.timers.write_byte(0xFF05, 0x04);
    address_bus.timers.write_byte(0xFF06, 0x02);
    address_bus.timers.write_byte(0xFF07, 0x07);

    address_bus.gpu.set_lcdc(0x80);
    address_bus.gpu.write_byte(0xFF42, 0x55);
    address_bus.gpu.write_byte(0xFF43, 0xA1);
    address_bus.gpu.write_byte(0xFF4A, 0xBB);
    address_bus.gpu.write_byte(0xFF4B, 0xDD);
    address_bus.gpu.palettes_mut().set_bgp(0xC1);
    address_bus.gpu.set_frame_position(0x2B, 300);
    address_bus.gpu.write_byte(0xFF45, 0xAB);
    address_bus.gpu.write_byte(0xFF41, 0xD2);
    address_bus.gpu.palettes_mut().set_obp0(0x1B);
    address_bus.gpu.palettes_mut().set_obp1(0xE4);
    address_bus.gpu.set_stat_interrupt(true);

    address_bus.joypad.set_group_select(0x10);
    address_bus.joypad.set_action_buttons(0x4);

    for (address, value) in [
        (0xFF26, 0x80),
        (0xFF25, 0xF2),
        (0xFF24, 0xC1),
        (0xFF10, 0xDD),
        (0xFF11, 0xB0),
        (0xFF12, 0xAA),
        (0xFF13, 0xB2),
        (0xFF14, 0x42),
        (0xFF16, 0xC0),
        (0xFF17, 0xC1),
        (0xFF18, 0x14),
        (0xFF19, 0x24),
        (0xFF1A, 0x80),
        (0xFF1C, 0x60),
        (0xFF1D, 0xFF),
        (0xFF1E, 0x44),
        (0xFF30, 0xB1),
        (0xFF31, 0xD2),
        (0xFF20, 0x1A),
        (0xFF21, 0xD2),
        (0xFF22, 0xCE),
        (0xFF23, 0x40),
    ] {
        address_bus.apu.write_byte(address, value);
    }

    address_bus.serial.set_interrupt(true);

    address_bus.in_boot_rom = false;

    address_bus
}

#[test]
fn reads_from_bios() {
    let mut address_bus = setup_test_address_bus();
    address_bus.in_boot_rom = true;
    assert_eq!(address_bus.peek(0x02), 0x03);
}

#[test]
fn game_genie_codes_change_the_cartridge_but_not_the_boot_rom() {
    let mut address_bus = setup_test_address_bus();
    address_bus
        .cheats_mut()
        .register_gamegenie_cheat("boot", "420-02F")
        .unwrap();
    assert_eq!(address_bus.peek(0x02), 0x42);
    address_bus.in_boot_rom = true;
    assert_eq!(address_bus.peek(0x02), 0x03);
}

#[test]
fn reads_from_rom_in_bank_zero() {
    let address_bus = setup_test_address_bus();
    assert_eq!(address_bus.peek(0x02), 0x01);
}

#[test]
fn reads_from_rom_in_bank_zero_scenario_two() {
    let address_bus = setup_test_address_bus();
    assert_eq!(address_bus.peek(0x20B1), 0xEE);
}

#[test]
fn reads_from_rom_in_subsequent_bank() {
    let address_bus = setup_test_address_bus();
    assert_eq!(address_bus.peek(0x5ACE), 0x55);
}

#[test]
fn reads_from_video_ram() {
    let address_bus = setup_test_address_bus();
    assert_eq!(address_bus.peek(0x8002), 0xAA);
}

#[test]
fn disallow_access_to_external_ram_if_not_enabled() {
    let address_bus = setup_test_address_bus();
    assert_eq!(address_bus.peek(0xA001), 0xFF);
}

#[test]
fn reads_from_working_ram() {
    let address_bus = setup_test_address_bus();
    assert_eq!(address_bus.peek(0xC002), 0x2B);
}

#[test]
fn reads_from_working_ram_shadow() {
    let address_bus = setup_test_address_bus();
    assert_eq!(address_bus.peek(0xE002), 0x2B);
}

#[test]
fn reads_from_working_ram_shadow_scenario_two() {
    let address_bus = setup_test_address_bus();
    assert_eq!(address_bus.peek(0xF5F0), 0x2B);
}

#[test]
fn reads_from_separate_working_ram_bank() {
    let mut address_bus = setup_test_address_bus();
    address_bus.set_model(Model::CgbE);
    address_bus.working_ram[0x2001] = 0x22;

    address_bus.poke(0xFF70, 0x02);

    assert_eq!(address_bus.peek(0xD001), 0x22);
    assert_eq!(address_bus.peek(0xF001), 0x22);
}

#[test]
fn reads_from_object_attribute_memory() {
    let address_bus = setup_test_address_bus();
    assert_eq!(address_bus.peek(0xFE7B), 0x45);
}

#[test]
fn reads_from_high_ram() {
    let address_bus = setup_test_address_bus();
    assert_eq!(address_bus.peek(0xFFA0), 0xBB);
}

#[test]
fn reads_from_interrupts_enabled_register() {
    let address_bus = setup_test_address_bus();
    assert_eq!(address_bus.peek(0xFFFF), 0x1F);
}

#[test]
fn reads_from_interrupt_flags_register() {
    let address_bus = setup_test_address_bus();
    assert_eq!(address_bus.peek(0xFF0F), 0xEA);
}

#[test]
fn reads_from_timer_divider_register() {
    let address_bus = setup_test_address_bus();
    assert_eq!(address_bus.peek(0xFF04), 0x3A);
}

#[test]
fn reads_from_timer_counter_register() {
    let address_bus = setup_test_address_bus();
    assert_eq!(address_bus.peek(0xFF05), 0x04);
}

#[test]
fn reads_from_timer_modulo_register() {
    let address_bus = setup_test_address_bus();
    assert_eq!(address_bus.peek(0xFF06), 0x02);
}

#[test]
fn reads_from_timer_control_register() {
    let address_bus = setup_test_address_bus();
    assert_eq!(address_bus.peek(0xFF07), 0xFF);
}

#[test]
fn reads_lcdc_register_from_gpu() {
    let address_bus = setup_test_address_bus();
    assert_eq!(address_bus.peek(0xFF40), 0x80);
}

#[test]
fn reads_scroll_registers_from_gpu() {
    let address_bus = setup_test_address_bus();
    assert_eq!(address_bus.peek(0xFF42), 0x55);
    assert_eq!(address_bus.peek(0xFF43), 0xA1);
}

#[test]
fn reads_window_position_registers_from_gpu() {
    let address_bus = setup_test_address_bus();
    assert_eq!(address_bus.peek(0xFF4A), 0xBB);
    assert_eq!(address_bus.peek(0xFF4B), 0xDD);
}

#[test]
fn reads_palette_from_gpu() {
    let address_bus = setup_test_address_bus();
    assert_eq!(address_bus.peek(0xFF47), 0xC1);
}

#[test]
fn reads_ly_from_gpu() {
    let address_bus = setup_test_address_bus();
    assert_eq!(address_bus.peek(0xFF44), 0x2B);
}

#[test]
fn reads_lyc_from_gpu() {
    let address_bus = setup_test_address_bus();
    assert_eq!(address_bus.peek(0xFF45), 0xAB);
}

#[test]
fn reads_stat_from_gpu() {
    let address_bus = setup_test_address_bus();
    assert_eq!(address_bus.peek(0xFF41), 0xD0);
}

#[test]
fn reads_obp0_from_gpu() {
    let address_bus = setup_test_address_bus();
    assert_eq!(address_bus.peek(0xFF48), 0x1B);
}

#[test]
fn reads_obp1_from_gpu() {
    let address_bus = setup_test_address_bus();
    assert_eq!(address_bus.peek(0xFF49), 0xE4);
}

#[test]
fn writes_obp0_value_to_gpu() {
    let mut address_bus = setup_test_address_bus();
    address_bus.poke(0xFF48, 0xE4);
    assert_eq!(address_bus.peek(0xFF48), 0xE4);
}

#[test]
fn writes_obp1_value_to_gpu() {
    let mut address_bus = setup_test_address_bus();
    address_bus.poke(0xFF49, 0x1B);
    assert_eq!(address_bus.peek(0xFF49), 0x1B);
}

#[test]
fn reads_joyp_register() {
    let address_bus = setup_test_address_bus();
    assert_eq!(address_bus.peek(0xFF00), 0xD4);
}

#[test]
fn loads_rom_buffer_into_emulator() {
    let mut address_bus = setup_test_address_bus();

    let mut rom = build_rom(CARTRIDGE_TYPE_MBC1, ROM_SIZE_64KB, RAM_SIZE_2KB);
    rom[0] = 0xA0;
    rom[1] = 0xCC;
    rom[2] = 0x3B;
    rom[3] = 0x4C;
    rom[0x146] = 0x00;
    rom[0x147] = 0x01;
    rom[0x7FFF] = 0xD4;
    rom[0x8000] = 0xBB;
    rom[0x8001] = 0xD1;
    let header = address_bus
        .load_rom(rom, empty_cartridge_effects())
        .unwrap();

    assert_eq!(address_bus.peek(0x0000), 0xA0);
    assert_eq!(address_bus.peek(0x0001), 0xCC);
    assert_eq!(address_bus.peek(0x0002), 0x3B);
    assert_eq!(address_bus.peek(0x0003), 0x4C);
    assert_eq!(address_bus.peek(0x7FFF), 0xD4);
    assert_eq!(address_bus.peek(0x8000), 0xB1);

    assert!(!header.supports_sgb);
    assert_eq!(header.cartridge_type, 0x01);
}

#[test]
fn writes_to_video_ram() {
    let mut address_bus = setup_test_address_bus();
    address_bus.poke(0x8002, 0xC1);
    assert_eq!(address_bus.gpu.video_ram_byte(2), 0xC1);
}

#[test]
fn reads_from_audio_master_control() {
    let address_bus = setup_test_address_bus();
    assert_eq!(address_bus.peek(0xFF26), 0xF0);
}

#[test]
fn writes_to_audio_master_control() {
    let mut address_bus = setup_test_address_bus();
    address_bus.poke(0xFF26, 0x0);
    assert_eq!(address_bus.peek(0xFF26), 0x70);
}

#[test]
fn reads_from_sound_panning() {
    let address_bus = setup_test_address_bus();
    assert_eq!(address_bus.peek(0xFF25), 0xF2);
}

#[test]
fn reads_from_master_volume() {
    let address_bus = setup_test_address_bus();
    assert_eq!(address_bus.peek(0xFF24), 0xC1);
}

#[test]
fn reads_from_ch1_sweep() {
    let address_bus = setup_test_address_bus();
    assert_eq!(address_bus.peek(0xFF10), 0xDD);
}

#[test]
fn reads_from_ch1_length_and_duty() {
    let address_bus = setup_test_address_bus();
    assert_eq!(address_bus.peek(0xFF11), 0xBF);
}

#[test]
fn reads_from_ch1_volume() {
    let address_bus = setup_test_address_bus();
    assert_eq!(address_bus.peek(0xFF12), 0xAA);
}

#[test]
fn reads_from_ch1_period_low() {
    let address_bus = setup_test_address_bus();
    assert_eq!(address_bus.peek(0xFF13), 0xFF);
}

#[test]
fn reads_from_ch1_period_high() {
    let address_bus = setup_test_address_bus();
    assert_eq!(address_bus.peek(0xFF14), 0xFF);
}

#[test]
fn reads_from_ch2_length_and_duty() {
    let address_bus = setup_test_address_bus();
    assert_eq!(address_bus.peek(0xFF16), 0xFF);
}

#[test]
fn reads_from_ch2_volume() {
    let address_bus = setup_test_address_bus();
    assert_eq!(address_bus.peek(0xFF17), 0xC1);
}

#[test]
fn reads_from_ch2_period_low() {
    let address_bus = setup_test_address_bus();
    assert_eq!(address_bus.peek(0xFF18), 0xFF);
}

#[test]
fn reads_from_ch2_period_high() {
    let address_bus = setup_test_address_bus();
    assert_eq!(address_bus.peek(0xFF19), 0xBF);
}

#[test]
fn reads_from_ch3_dac_enabled() {
    let address_bus = setup_test_address_bus();
    assert_eq!(address_bus.peek(0xFF1A), 0xFF);
}

#[test]
fn reads_from_ch3_output() {
    let address_bus = setup_test_address_bus();
    assert_eq!(address_bus.peek(0xFF1C), 0xFF);
}

#[test]
fn reads_from_ch3_period_high() {
    let address_bus = setup_test_address_bus();
    assert_eq!(address_bus.peek(0xFF1E), 0xFF);
}

#[test]
fn reads_from_wave_pattern_ram() {
    let address_bus = setup_test_address_bus();
    assert_eq!(address_bus.peek(0xFF30), 0xB1);
    assert_eq!(address_bus.peek(0xFF31), 0xD2);
}

#[test]
fn reads_from_ch4_length() {
    let address_bus = setup_test_address_bus();
    assert_eq!(address_bus.peek(0xFF20), 0xFF);
}

#[test]
fn reads_from_ch4_envelope() {
    let address_bus = setup_test_address_bus();
    assert_eq!(address_bus.peek(0xFF21), 0xD2);
}

#[test]
fn reads_from_ch4_polynomial() {
    let address_bus = setup_test_address_bus();
    assert_eq!(address_bus.peek(0xFF22), 0xCE);
}

#[test]
fn reads_from_ch4_control() {
    let address_bus = setup_test_address_bus();
    assert_eq!(address_bus.peek(0xFF23), 0xFF);
}

#[test]
fn reads_from_key1() {
    let mut address_bus = setup_test_address_bus();
    address_bus.set_model(Model::CgbE);
    assert_eq!(address_bus.peek(0xFF4D), 0x7E);
}

fn address_bus_with_model(model: Model) -> AddressBus {
    let mut address_bus = initialize_test_address_bus();
    address_bus.set_model(model);
    address_bus
}

#[test]
fn ignores_vbk_and_opri_on_dmg() {
    let mut address_bus = address_bus_with_model(Model::Dmg);
    address_bus.poke(0xFF4F, 0x01);
    address_bus.poke(0xFF6C, 0x01);

    assert_eq!(address_bus.peek(0xFF4F), 0xFF);
    assert_eq!(address_bus.peek(0xFF6C), 0xFF);
}

#[test]
fn only_cgb_mode_picks_the_fast_serial_clock() {
    for (key0, high_speed_bit) in [(0x00, 0x02), (0x04, 0x00)] {
        let mut address_bus = address_bus_with_model(Model::CgbE);
        address_bus.in_boot_rom = true;
        address_bus.poke(0xFF4C, key0);
        address_bus.in_boot_rom = false;
        address_bus.poke(0xFF02, 0x83);
        assert_eq!(address_bus.serial.read_byte(0xFF02) & 0x02, high_speed_bit);
    }
}

#[test]
fn reads_zero_from_unusable_oam_on_dmg() {
    let mut address_bus = address_bus_with_model(Model::Dmg);
    address_bus.poke(0xFEA5, 0x12);
    assert_eq!(address_bus.peek(0xFEA5), 0x00);
}

#[test]
fn reads_an_address_pattern_from_unusable_oam_on_cgb_e() {
    let mut address_bus = address_bus_with_model(Model::CgbE);
    address_bus.poke(0xFEC4, 0x12);
    assert_eq!(address_bus.peek(0xFEC4), 0xCC);
}

#[test]
fn mirrors_unusable_oam_every_32_bytes_on_cgb_c() {
    let mut address_bus = address_bus_with_model(Model::CgbC);
    address_bus.poke(0xFEA1, 0x34);
    assert_eq!(address_bus.peek(0xFEB9), 0x34);
    assert_eq!(address_bus.peek(0xFEE1), 0x00);
}

#[test]
fn mirrors_the_upper_unusable_oam_on_cgb_d() {
    let mut address_bus = address_bus_with_model(Model::CgbD);
    address_bus.poke(0xFEC2, 0x56);
    assert_eq!(address_bus.peek(0xFEF2), 0x56);
    assert_eq!(address_bus.peek(0xFED2), 0x56);
    assert_eq!(address_bus.peek(0xFEA2), 0x00);
}

#[test]
fn reads_and_writes_the_last_address_in_processor_test_mode() {
    let mut address_bus = AddressBus::new(true);
    address_bus.poke(0xFFFF, 0x12);
    assert_eq!(address_bus.peek(0xFFFF), 0x12);
    assert_eq!(address_bus.processor_test_ram()[0xFFFF], 0x12);
}

#[test]
fn powers_on_monochrome_work_ram_in_alternating_blocks() {
    let mut address_bus = initialize_test_address_bus();
    address_bus.set_model(Model::Dmg);

    assert_eq!(address_bus.working_ram[0x0000], 0xFF);
    assert_eq!(address_bus.working_ram[0x0100], 0x00);
    assert_eq!(address_bus.working_ram[0x1E9F], 0xFF);
    assert_eq!(address_bus.working_ram[0x1F45], 0x00);
}

#[test]
fn powers_on_cgb_work_ram_with_noise() {
    let mut address_bus = initialize_test_address_bus();
    address_bus.set_model(Model::CgbE);

    let noisy_bytes = address_bus
        .working_ram
        .iter()
        .filter(|&&byte| byte != 0x00 && byte != 0xFF)
        .count();
    assert!(noisy_bytes > address_bus.working_ram.len() / 2);
}

#[test]
fn dots_the_ppu_ran_too_far_ahead_carry_into_the_next_m_cycle() {
    let mut address_bus = initialize_test_address_bus();
    address_bus.ppu_dot_debt = -1;

    assert_eq!(address_bus.take_cycle_dots(2, 2), 0);
    assert_eq!(address_bus.ppu_dot_debt, -1);
    assert_eq!(address_bus.take_cycle_dots(2, 0), 1);
    assert_eq!(address_bus.ppu_dot_debt, 0);
}

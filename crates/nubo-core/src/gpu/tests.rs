//! Unit tests for the PPU line timeline, STAT, register access and save states.

use crate::address_bus::MemoryMapped;
use crate::address_bus::hdma::HdmaState;
use crate::gpu::Gpu;
use crate::model::Model;
use crate::serializable::Serializable;

fn initialize_test_gpu() -> Gpu {
    let mut gpu = Gpu::new();
    gpu.set_lcdc(0b1000_0000);
    gpu
}

fn gpu_at(line: u8, dot: u16) -> Gpu {
    let mut gpu = initialize_test_gpu();
    gpu.set_frame_position(line, dot);
    gpu
}

fn tick_dots(gpu: &mut Gpu, dots: u16) {
    let mut hdma = HdmaState::new();
    for _ in 0..dots {
        gpu.tick_dot(&mut hdma);
    }
}

#[test]
fn should_start_first_line_after_lcd_on_in_mode_0() {
    let mut gpu = initialize_test_gpu();
    assert_eq!(gpu.ly, 0);
    assert_eq!(gpu.mode(), 0);
    assert_eq!(gpu.line_dot, 3);

    tick_dots(&mut gpu, 80);
    assert_eq!(gpu.mode(), 0);

    tick_dots(&mut gpu, 1);
    assert_eq!(gpu.mode(), 3);
}

#[test]
fn should_increment_ly_at_the_start_of_a_line_and_show_mode_2_a_dot_later() {
    let mut gpu = gpu_at(5, 455);
    tick_dots(&mut gpu, 1);
    assert_eq!(gpu.ly, 6);
    assert_eq!(gpu.mode(), 0);

    tick_dots(&mut gpu, 1);
    assert_eq!(gpu.mode(), 2);
}

#[test]
fn should_draw_for_169_dots_without_scrolling_or_sprites() {
    let mut gpu = gpu_at(1, 0);
    tick_dots(&mut gpu, 84);
    assert_eq!(gpu.mode(), 3);

    tick_dots(&mut gpu, 168);
    assert_eq!(gpu.mode(), 3);

    tick_dots(&mut gpu, 1);
    assert_eq!(gpu.mode(), 0);
}

#[test]
fn should_extend_drawing_by_the_fine_horizontal_scroll() {
    let mut gpu = gpu_at(1, 0);
    gpu.scx = 5;
    tick_dots(&mut gpu, 84 + 168 + 5);
    assert_eq!(gpu.mode(), 3);

    tick_dots(&mut gpu, 5);
    assert_eq!(gpu.mode(), 0);
}

#[test]
fn should_enter_vblank_and_complete_the_frame_at_line_144() {
    let mut gpu = gpu_at(143, 455);
    tick_dots(&mut gpu, 1);
    assert_eq!(gpu.ly, 144);
    assert!(gpu.frame_ready);
    assert_eq!(gpu.mode(), 0);

    tick_dots(&mut gpu, 1);
    assert!(!gpu.vblank_interrupt);

    tick_dots(&mut gpu, 1);
    assert_eq!(gpu.mode(), 1);
    assert!(gpu.vblank_interrupt);
}

#[test]
fn should_reset_ly_early_in_line_153() {
    let mut gpu = gpu_at(152, 455);
    tick_dots(&mut gpu, 1);
    assert_eq!(gpu.ly, 153);

    tick_dots(&mut gpu, 4);
    assert_eq!(gpu.ly, 0);

    tick_dots(&mut gpu, 451);
    assert_eq!(gpu.ly, 0);
    assert_eq!(gpu.mode(), 1);

    tick_dots(&mut gpu, 1);
    assert_eq!(gpu.ly, 0);
    assert_eq!(gpu.mode(), 0);

    tick_dots(&mut gpu, 4);
    assert_eq!(gpu.mode(), 2);
}

#[test]
fn should_update_lyc_flag_two_dots_into_a_line_on_dmg() {
    let mut gpu = gpu_at(5, 455);
    gpu.write_byte(0xFF45, 6);

    tick_dots(&mut gpu, 2);
    assert_eq!(gpu.read_byte(0xFF41) & 0b100, 0);

    tick_dots(&mut gpu, 1);
    assert_eq!(gpu.read_byte(0xFF41) & 0b100, 0b100);
}

#[test]
fn should_request_mode_2_stat_interrupt_when_ly_changes() {
    let mut gpu = gpu_at(5, 454);
    gpu.write_byte(0xFF41, 0b0010_0000);
    gpu.stat_interrupt = false;

    tick_dots(&mut gpu, 1);
    assert!(!gpu.stat_interrupt);

    tick_dots(&mut gpu, 1);
    assert!(gpu.stat_interrupt);
    assert_eq!(gpu.mode(), 0);
}

#[test]
fn should_not_request_stat_interrupt_while_the_stat_line_stays_high() {
    let mut gpu = gpu_at(5, 300);
    gpu.write_byte(0xFF45, 6);
    gpu.write_byte(0xFF41, 0b0110_1000);
    gpu.stat_interrupt = false;

    tick_dots(&mut gpu, 156 + 4);
    assert!(!gpu.stat_interrupt);
}

#[test]
fn should_request_stat_interrupt_when_a_source_is_enabled_during_its_mode() {
    let mut gpu = gpu_at(150, 100);
    gpu.set_model(Model::CgbE);
    gpu.stat_interrupt = false;

    gpu.write_byte(0xFF41, 0b0001_0000);
    assert!(gpu.stat_interrupt);
}

#[test]
fn should_ignore_writes_to_read_only_stat_bits() {
    let mut gpu = gpu_at(150, 100);
    gpu.write_byte(0xFF41, 0b0000_0111);
    assert_eq!(gpu.read_byte(0xFF41), 0b1000_0001);
}

#[test]
fn should_block_cpu_access_to_oam_and_vram_while_scanning_and_drawing() {
    let mut gpu = gpu_at(1, 0);
    assert!(gpu.oam_read_blocked());
    assert!(!gpu.oam_write_blocked());
    assert!(!gpu.vram_read_blocked());

    tick_dots(&mut gpu, 4);
    assert!(gpu.oam_write_blocked());

    tick_dots(&mut gpu, 76);
    assert!(!gpu.oam_write_blocked());
    assert!(gpu.vram_read_blocked());
    assert!(!gpu.vram_write_blocked());

    tick_dots(&mut gpu, 4);
    assert!(gpu.oam_write_blocked());
    assert!(gpu.vram_write_blocked());

    tick_dots(&mut gpu, 172);
    assert!(!gpu.oam_read_blocked());
    assert!(!gpu.vram_read_blocked());
}

#[test]
fn should_reset_ly_and_keep_lyc_flag_when_lcd_is_turned_off() {
    let mut gpu = gpu_at(20, 100);
    gpu.write_byte(0xFF45, 20);
    gpu.set_lcdc(0);

    assert_eq!(gpu.ly, 0);
    assert_eq!(gpu.read_byte(0xFF41), 0b1000_0100);
}

#[test]
fn should_set_cgb_vbk() {
    let mut gpu = initialize_test_gpu();
    gpu.set_model(Model::CgbE);
    gpu.set_cgb_vbk(1);
    assert_eq!(gpu.cgb_vbk, 1);
}

#[test]
fn should_get_cgb_vbk() {
    let mut gpu = initialize_test_gpu();
    gpu.set_model(Model::CgbE);
    gpu.cgb_vbk = 1;
    assert_eq!(gpu.cgb_vbk(), 0xFF);
}

#[test]
fn should_ignore_all_bits_other_than_bit_0_when_getting_cgb_vbk() {
    let mut gpu = initialize_test_gpu();
    gpu.set_model(Model::CgbE);
    gpu.cgb_vbk = 0b0010_1010;
    assert_eq!(gpu.cgb_vbk(), 0b1111_1110);
}

#[test]
fn should_read_from_bank_1_of_video_ram() {
    let mut gpu = initialize_test_gpu();
    gpu.set_model(Model::CgbE);
    gpu.video_ram[0x3800] = 0xA1;
    gpu.set_cgb_vbk(1);
    assert_eq!(gpu.video_ram_byte(0x1800), 0xA1);
}

#[test]
fn should_set_byte_in_bank_1_of_video_ram() {
    let mut gpu = initialize_test_gpu();
    gpu.set_model(Model::CgbE);
    gpu.set_cgb_vbk(1);
    gpu.set_video_ram_byte(0x1802, 0xA1);
    assert_eq!(gpu.video_ram[0x3802], 0xA1);
}

#[test]
fn should_read_from_bank_0_of_video_ram() {
    let mut gpu = initialize_test_gpu();
    gpu.set_model(Model::CgbE);
    gpu.video_ram[0x1800] = 0xA1;
    gpu.set_cgb_vbk(0);
    assert_eq!(gpu.video_ram_byte(0x1800), 0xA1);
}

#[test]
fn should_set_byte_in_bank_0_of_video_ram() {
    let mut gpu = initialize_test_gpu();
    gpu.set_model(Model::CgbE);
    gpu.set_cgb_vbk(0);
    gpu.set_video_ram_byte(0x1802, 0xA1);
    assert_eq!(gpu.video_ram[0x1802], 0xA1);
}

fn serialize(gpu: &Gpu) -> Vec<u8> {
    let mut bytes = Vec::new();
    gpu.serialize(&mut bytes).unwrap();
    bytes
}

#[test]
fn should_restore_the_frame_position_from_a_save_state() {
    let bytes = serialize(&gpu_at(20, 100));

    let mut restored = Gpu::new();
    restored.deserialize(&mut bytes.as_slice()).unwrap();

    assert_eq!(restored.line, 20);
    assert_eq!(restored.line_dot, 100);
    assert_eq!(restored.ly, 20);
}

#[test]
fn should_reject_a_save_state_with_a_line_outside_the_frame() {
    let mut bytes = serialize(&gpu_at(20, 100));
    bytes[0] = 0xFF;

    assert!(Gpu::new().deserialize(&mut bytes.as_slice()).is_err());
}

#[test]
fn should_reject_a_save_state_with_a_dot_outside_the_line() {
    let mut bytes = serialize(&gpu_at(20, 100));
    bytes[1..3].copy_from_slice(&456u16.to_le_bytes());

    assert!(Gpu::new().deserialize(&mut bytes.as_slice()).is_err());
}

#[test]
fn should_reject_a_save_state_comparing_lyc_with_a_line_outside_the_frame() {
    let mut gpu = gpu_at(20, 100);
    gpu.lyc_source_line = Some(153);
    let bytes = serialize(&gpu);
    assert!(Gpu::new().deserialize(&mut bytes.as_slice()).is_ok());

    gpu.lyc_source_line = Some(154);
    let bytes = serialize(&gpu);
    assert!(Gpu::new().deserialize(&mut bytes.as_slice()).is_err());
}

/// Ticks `gpu` to the end of the current line and returns the dots of the line at which a STAT
/// interrupt was requested, and the dot at which STAT first showed mode 0.
fn stat_requests_until_line_end(gpu: &mut Gpu) -> (Vec<u16>, Option<u16>) {
    let mut hdma = HdmaState::new();
    let line = gpu.line;
    let mut requests = Vec::new();
    let mut hblank_dot = None;
    while gpu.line == line {
        gpu.stat_interrupt = false;
        gpu.tick_dot(&mut hdma);
        if gpu.line != line {
            break;
        }
        if gpu.stat_interrupt {
            requests.push(gpu.line_dot);
        }
        if hblank_dot.is_none() && gpu.line_dot > 84 && gpu.mode() == 0 {
            hblank_dot = Some(gpu.line_dot);
        }
    }
    (requests, hblank_dot)
}

#[test]
fn should_request_one_mode_0_interrupt_when_an_object_fetch_delays_the_last_pixel() {
    let mut gpu = gpu_at(1, 0);
    gpu.set_lcdc(0b1000_0010);
    for object in 0..10 {
        gpu.object_attribute_memory[object * 4] = 16;
        gpu.object_attribute_memory[object * 4 + 1] = 167;
    }
    gpu.write_byte(0xFF41, 0b0000_1000);

    let (requests, hblank_dot) = stat_requests_until_line_end(&mut gpu);

    assert_eq!(hblank_dot, Some(313));
    assert_eq!(requests, vec![254]);
}

#[test]
fn should_compare_lyc_against_153_then_nothing_then_0_on_line_153() {
    let comparisons = |double_speed: bool| {
        let mut gpu = gpu_at(153, 0);
        gpu.set_model(Model::CgbE);
        gpu.set_cgb_double_speed(double_speed);
        (0..12)
            .map(|dot| {
                gpu.line_dot = dot;
                gpu.ly_for_comparison()
            })
            .collect::<Vec<_>>()
    };
    let (none, line_152, line_153, zero) = (None, Some(152), Some(153), Some(0));

    assert_eq!(
        comparisons(false),
        [
            none, none, line_153, line_153, line_153, line_153, line_153, line_153, none, none,
            zero, zero
        ]
    );
    assert_eq!(
        comparisons(true),
        [
            line_152, line_152, line_153, line_153, line_153, line_153, line_153, line_153,
            line_153, none, zero, zero
        ]
    );
}

#[test]
fn should_reject_a_save_state_without_the_super_game_boy_its_model_has() {
    let bytes = serialize(&Gpu::new());
    let mut sgb = Gpu::new();
    sgb.set_model(Model::Sgb);
    assert!(sgb.deserialize(&mut bytes.as_slice()).is_err());

    let mut with_sgb = Gpu::new();
    with_sgb.set_model(Model::Sgb);
    let bytes = serialize(&with_sgb);
    assert!(Gpu::new().deserialize(&mut bytes.as_slice()).is_err());
}

mod random_scenes;

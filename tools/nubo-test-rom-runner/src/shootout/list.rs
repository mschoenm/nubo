//! The Shootout's tests at the commit `download.rs` pins, in its order,
//! generated once from its `testroms/*.py` and `main.py` without running
//! them (see the module docs of `shootout.rs`).
//!
//! Derived from the GB Emulator Shootout
//! (<https://github.com/gbdev/GBEmulatorShootout>), MIT License, Copyright
//! (c) 2026 daid; see `NOTICE`.
//!
//! Left out: the Super Game Boy tests `samesuite/sgb/command_mlt_req.gb`,
//! `samesuite/sgb/command_mlt_req_1_incrementing.gb` and
//! `cpp/sgb-ext-test.gb`, and `acid/which.gb (DMG)`, `acid/which.gb (GBC)`
//! and `daid/rom_and_ram.gb`, which have no reference screenshot. The
//! Shootout's `required_features=(PCM,)` is left out too: Nubo has the PCM12
//! and PCM34 registers, so it runs those tests.

use super::{CGB, DMG, ShootoutTest, test};

/// Every Shootout test the runner runs.
pub(super) static TESTS: [ShootoutTest; 258] = [
    test("acid/dmg-acid2.gb", DMG, 1500).in_collection("dmg-acid2/dmg-acid2.gb"),
    test("acid/cgb-acid2.gbc", CGB, 1500).in_collection("cgb-acid2/cgb-acid2.gbc"),
    test("acid/cgb-acid-hell.gbc", CGB, 1500).in_collection("cgb-acid-hell/cgb-acid-hell.gbc"),
    test("blargg/cpu_instrs/01-special.gb", DMG, 3000),
    test("blargg/cpu_instrs/02-interrupts.gb", DMG, 1000),
    test("blargg/cpu_instrs/03-op_sp,hl.gb", DMG, 2500),
    test("blargg/cpu_instrs/04-op_r,imm.gb", DMG, 2500),
    test("blargg/cpu_instrs/05-op_rp.gb", DMG, 3000),
    test("blargg/cpu_instrs/06-ld_r,r.gb", DMG, 1000),
    test("blargg/cpu_instrs/07-jr,jp,call,ret,rst.gb", DMG, 1000),
    test("blargg/cpu_instrs/08-misc_instrs.gb", DMG, 1000),
    test("blargg/cpu_instrs/09-op_r,r.gb", DMG, 7000),
    test("blargg/cpu_instrs/10-bit_ops.gb", DMG, 14000),
    test("blargg/cpu_instrs/11-op_a,(hl).gb", DMG, 13000),
    test("blargg/halt_bug.gb", DMG, 2000),
    test("blargg/instr_timing.gb", DMG, 1000),
    test("blargg/interrupt_time.gb", CGB, 1500)
        .in_collection("blargg/interrupt_time/interrupt_time.gb"),
    test("blargg/mem_timing/01-read_timing.gb", DMG, 1000),
    test("blargg/mem_timing/02-write_timing.gb", DMG, 1000),
    test("blargg/mem_timing/03-modify_timing.gb", DMG, 1000),
    test("blargg/mem_timing-2/01-read_timing.gb", DMG, 1000),
    test("blargg/mem_timing-2/02-write_timing.gb", DMG, 1000),
    test("blargg/mem_timing-2/03-modify_timing.gb", DMG, 1000),
    test("blargg/oam_bug/1-lcd_sync.gb", DMG, 1000),
    test("blargg/oam_bug/2-causes.gb", DMG, 1000),
    test("blargg/oam_bug/3-non_causes.gb", DMG, 3000),
    test("blargg/oam_bug/4-scanline_timing.gb", DMG, 1500),
    test("blargg/oam_bug/5-timing_bug.gb", DMG, 1500),
    test("blargg/oam_bug/6-timing_no_bug.gb", DMG, 1500),
    test("blargg/oam_bug/8-instr_effect.gb", DMG, 1500),
    test("blargg/dmg_sound/01-registers.gb", DMG, 2000)
        .in_collection("blargg/dmg_sound/rom_singles/01-registers.gb"),
    test("blargg/dmg_sound/02-len_ctr.gb", DMG, 11000)
        .in_collection("blargg/dmg_sound/rom_singles/02-len ctr.gb"),
    test("blargg/dmg_sound/03-trigger.gb", DMG, 18000)
        .in_collection("blargg/dmg_sound/rom_singles/03-trigger.gb"),
    test("blargg/dmg_sound/04-sweep.gb", DMG, 2000)
        .in_collection("blargg/dmg_sound/rom_singles/04-sweep.gb"),
    test("blargg/dmg_sound/05-sweep_details.gb", DMG, 2000)
        .in_collection("blargg/dmg_sound/rom_singles/05-sweep details.gb"),
    test("blargg/dmg_sound/06-overflow_on_trigger.gb", DMG, 2000)
        .in_collection("blargg/dmg_sound/rom_singles/06-overflow on trigger.gb"),
    test("blargg/dmg_sound/07-len_sweep_period_sync.gb", DMG, 2000)
        .in_collection("blargg/dmg_sound/rom_singles/07-len sweep period sync.gb"),
    test("blargg/dmg_sound/08-len_ctr_during_power.gb", DMG, 2000)
        .in_collection("blargg/dmg_sound/rom_singles/08-len ctr during power.gb"),
    test("blargg/dmg_sound/09-wave_read_while_on.gb", DMG, 1000)
        .in_collection("blargg/dmg_sound/rom_singles/09-wave read while on.gb"),
    test("blargg/dmg_sound/10-wave_trigger_while_on.gb", DMG, 5000)
        .in_collection("blargg/dmg_sound/rom_singles/10-wave trigger while on.gb"),
    test("blargg/dmg_sound/11-regs_after_power.gb", DMG, 1000)
        .in_collection("blargg/dmg_sound/rom_singles/11-regs after power.gb"),
    test("blargg/dmg_sound/12-wave_write_while_on.gb", DMG, 5000)
        .in_collection("blargg/dmg_sound/rom_singles/12-wave write while on.gb"),
    test("blargg/cgb_sound/01-registers.gb", CGB, 1500)
        .in_collection("blargg/cgb_sound/rom_singles/01-registers.gb"),
    test("blargg/cgb_sound/02-len_ctr.gb", CGB, 11000)
        .in_collection("blargg/cgb_sound/rom_singles/02-len ctr.gb"),
    test("blargg/cgb_sound/03-trigger.gb", CGB, 18000)
        .in_collection("blargg/cgb_sound/rom_singles/03-trigger.gb"),
    test("blargg/cgb_sound/04-sweep.gb", CGB, 2500)
        .in_collection("blargg/cgb_sound/rom_singles/04-sweep.gb"),
    test("blargg/cgb_sound/05-sweep_details.gb", CGB, 12000)
        .in_collection("blargg/cgb_sound/rom_singles/05-sweep details.gb"),
    test("blargg/cgb_sound/06-overflow_on_trigger.gb", CGB, 2000)
        .in_collection("blargg/cgb_sound/rom_singles/06-overflow on trigger.gb"),
    test("blargg/cgb_sound/07-len_sweep_period_sync.gb", CGB, 1500)
        .in_collection("blargg/cgb_sound/rom_singles/07-len sweep period sync.gb"),
    test("blargg/cgb_sound/08-len_ctr_during_power.gb", CGB, 3500)
        .in_collection("blargg/cgb_sound/rom_singles/08-len ctr during power.gb"),
    test("blargg/cgb_sound/09-wave_read_while_on.gb", CGB, 1500)
        .in_collection("blargg/cgb_sound/rom_singles/09-wave read while on.gb"),
    test("blargg/cgb_sound/10-wave_trigger_while_on.gb", CGB, 5000)
        .in_collection("blargg/cgb_sound/rom_singles/10-wave trigger while on.gb"),
    test("blargg/cgb_sound/11-regs_after_power.gb", CGB, 1500)
        .in_collection("blargg/cgb_sound/rom_singles/11-regs after power.gb"),
    test("blargg/cgb_sound/12-wave.gb", CGB, 1500)
        .in_collection("blargg/cgb_sound/rom_singles/12-wave.gb"),
    test("daid/ppu_scanline_bgp.gb (DMG)", DMG, 500)
        .rom("daid/ppu_scanline_bgp.gb")
        .results(&[
            "daid/ppu_scanline_bgp_0.dmg.png",
            "daid/ppu_scanline_bgp_1.dmg.png",
            "daid/ppu_scanline_bgp_2.dmg.png",
        ]),
    test("daid/ppu_scanline_bgp.gb (GBC)", CGB, 500)
        .rom("daid/ppu_scanline_bgp.gb")
        .results(&["daid/ppu_scanline_bgp.gbc.png"]),
    test("daid/stop_instr.gb (DMG)", DMG, 500)
        .rom("daid/stop_instr.gb")
        .results(&["daid/stop_instr.dmg.png"]),
    test("daid/stop_instr.gb (GBC)", CGB, 500)
        .rom("daid/stop_instr.gb")
        .results(&["daid/stop_instr.gbc.png"]),
    test("daid/stop_instr_gbc_mode3.gb", CGB, 500),
    test("daid/speed_switch_timing_div.gbc", CGB, 500),
    test("daid/speed_switch_timing_ly.gbc", CGB, 500),
    test("daid/speed_switch_timing_stat.gbc", CGB, 500),
    test("ax6/rtc3test-1.gb", CGB, 9500),
    test("ax6/rtc3test-2.gb", CGB, 7500),
    test("ax6/rtc3test-3.gb", CGB, 20000),
    test("mooneye/acceptance/add_sp_e_timing.gb", DMG, 1500),
    test("mooneye/acceptance/bits/mem_oam.gb", DMG, 1500),
    test("mooneye/acceptance/bits/reg_f.gb", DMG, 1500),
    test("mooneye/acceptance/bits/unused_hwio-GS.gb", DMG, 1500),
    test("mooneye/acceptance/boot_div-dmgABCmgb.gb", DMG, 1500),
    test("mooneye/acceptance/boot_hwio-dmgABCmgb.gb", DMG, 1500),
    test("mooneye/acceptance/boot_regs-dmgABC.gb", DMG, 1500),
    test("mooneye/acceptance/call_cc_timing.gb", DMG, 1500),
    test("mooneye/acceptance/call_cc_timing2.gb", DMG, 1500),
    test("mooneye/acceptance/call_timing.gb", DMG, 1500),
    test("mooneye/acceptance/call_timing2.gb", DMG, 1500),
    test("mooneye/acceptance/div_timing.gb", DMG, 1500),
    test("mooneye/acceptance/di_timing-GS.gb", DMG, 1500),
    test("mooneye/acceptance/ei_sequence.gb", DMG, 1500),
    test("mooneye/acceptance/ei_timing.gb", DMG, 1500),
    test("mooneye/acceptance/halt_ime0_ei.gb", DMG, 1500),
    test("mooneye/acceptance/halt_ime0_nointr_timing.gb", DMG, 1500),
    test("mooneye/acceptance/halt_ime1_timing.gb", DMG, 1500),
    test("mooneye/acceptance/halt_ime1_timing2-GS.gb", DMG, 1500),
    test("mooneye/acceptance/if_ie_registers.gb", DMG, 1500),
    test("mooneye/acceptance/instr/daa.gb", DMG, 1500),
    test("mooneye/acceptance/interrupts/ie_push.gb", DMG, 1500),
    test("mooneye/acceptance/intr_timing.gb", DMG, 1500),
    test("mooneye/acceptance/jp_cc_timing.gb", DMG, 1500),
    test("mooneye/acceptance/jp_timing.gb", DMG, 1500),
    test("mooneye/acceptance/ld_hl_sp_e_timing.gb", DMG, 1500),
    test("mooneye/acceptance/oam_dma/basic.gb", DMG, 1500),
    test("mooneye/acceptance/oam_dma/reg_read.gb", DMG, 1500),
    test("mooneye/acceptance/oam_dma/sources-GS.gb", DMG, 1500),
    test("mooneye/acceptance/oam_dma_restart.gb", DMG, 1500),
    test("mooneye/acceptance/oam_dma_start.gb", DMG, 1500),
    test("mooneye/acceptance/oam_dma_timing.gb", DMG, 1500),
    test("mooneye/acceptance/pop_timing.gb", DMG, 1500),
    test(
        "mooneye/acceptance/ppu/hblank_ly_scx_timing-GS.gb",
        DMG,
        1500,
    ),
    test("mooneye/acceptance/ppu/intr_1_2_timing-GS.gb", DMG, 1500),
    test("mooneye/acceptance/ppu/intr_2_0_timing.gb", DMG, 1500),
    test("mooneye/acceptance/ppu/intr_2_mode0_timing.gb", DMG, 1500),
    test(
        "mooneye/acceptance/ppu/intr_2_mode0_timing_sprites.gb",
        DMG,
        5500,
    ),
    test("mooneye/acceptance/ppu/intr_2_mode3_timing.gb", DMG, 1500),
    test("mooneye/acceptance/ppu/intr_2_oam_ok_timing.gb", DMG, 1500),
    test("mooneye/acceptance/ppu/lcdon_timing-GS.gb", DMG, 1500),
    test("mooneye/acceptance/ppu/lcdon_write_timing-GS.gb", DMG, 2000),
    test("mooneye/acceptance/ppu/stat_irq_blocking.gb", DMG, 1500),
    test("mooneye/acceptance/ppu/stat_lyc_onoff.gb", DMG, 1500),
    test("mooneye/acceptance/ppu/vblank_stat_intr-GS.gb", DMG, 1500),
    test("mooneye/acceptance/push_timing.gb", DMG, 1500),
    test("mooneye/acceptance/rapid_di_ei.gb", DMG, 1500),
    test("mooneye/acceptance/reti_intr_timing.gb", DMG, 1500),
    test("mooneye/acceptance/reti_timing.gb", DMG, 1500),
    test("mooneye/acceptance/ret_cc_timing.gb", DMG, 1500),
    test("mooneye/acceptance/ret_timing.gb", DMG, 1500),
    test("mooneye/acceptance/rst_timing.gb", DMG, 1500),
    test(
        "mooneye/acceptance/serial/boot_sclk_align-dmgABCmgb.gb",
        DMG,
        1500,
    ),
    test("mooneye/acceptance/timer/div_write.gb", DMG, 1500),
    test("mooneye/acceptance/timer/rapid_toggle.gb", DMG, 1500),
    test("mooneye/acceptance/timer/tim00.gb", DMG, 1500),
    test("mooneye/acceptance/timer/tim00_div_trigger.gb", DMG, 1500),
    test("mooneye/acceptance/timer/tim01.gb", DMG, 1500),
    test("mooneye/acceptance/timer/tim01_div_trigger.gb", DMG, 1500),
    test("mooneye/acceptance/timer/tim10.gb", DMG, 1500),
    test("mooneye/acceptance/timer/tim10_div_trigger.gb", DMG, 1500),
    test("mooneye/acceptance/timer/tim11.gb", DMG, 1500),
    test("mooneye/acceptance/timer/tim11_div_trigger.gb", DMG, 1500),
    test("mooneye/acceptance/timer/tima_reload.gb", DMG, 1500),
    test(
        "mooneye/acceptance/timer/tima_write_reloading.gb",
        DMG,
        1500,
    ),
    test("mooneye/acceptance/timer/tma_write_reloading.gb", DMG, 1500),
    test("mooneye/emulator-only/mbc1/bits_bank1.gb", DMG, 3500),
    test("mooneye/emulator-only/mbc1/bits_bank2.gb", DMG, 3500),
    test("mooneye/emulator-only/mbc1/bits_mode.gb", DMG, 4500),
    test("mooneye/emulator-only/mbc1/bits_ramg.gb", DMG, 6500),
    test("mooneye/emulator-only/mbc1/multicart_rom_8Mb.gb", DMG, 1000),
    test("mooneye/emulator-only/mbc1/ram_256kb.gb", DMG, 1500),
    test("mooneye/emulator-only/mbc1/ram_64kb.gb", DMG, 1500),
    test("mooneye/emulator-only/mbc1/rom_16Mb.gb", DMG, 1500),
    test("mooneye/emulator-only/mbc1/rom_1Mb.gb", DMG, 1500),
    test("mooneye/emulator-only/mbc1/rom_2Mb.gb", DMG, 1500),
    test("mooneye/emulator-only/mbc1/rom_4Mb.gb", DMG, 1500),
    test("mooneye/emulator-only/mbc1/rom_512kb.gb", DMG, 1500),
    test("mooneye/emulator-only/mbc1/rom_8Mb.gb", DMG, 1500),
    test("mooneye/emulator-only/mbc2/bits_ramg.gb", DMG, 7500),
    test("mooneye/emulator-only/mbc2/bits_romb.gb", DMG, 4000),
    test("mooneye/emulator-only/mbc2/bits_unused.gb", DMG, 3500),
    test("mooneye/emulator-only/mbc2/ram.gb", DMG, 1500),
    test("mooneye/emulator-only/mbc2/rom_1Mb.gb", DMG, 1500),
    test("mooneye/emulator-only/mbc2/rom_2Mb.gb", DMG, 1500),
    test("mooneye/emulator-only/mbc2/rom_512kb.gb", DMG, 1500),
    test("mooneye/emulator-only/mbc5/rom_16Mb.gb", DMG, 1500),
    test("mooneye/emulator-only/mbc5/rom_1Mb.gb", DMG, 1500),
    test("mooneye/emulator-only/mbc5/rom_2Mb.gb", DMG, 1500),
    test("mooneye/emulator-only/mbc5/rom_32Mb.gb", DMG, 1500),
    test("mooneye/emulator-only/mbc5/rom_4Mb.gb", DMG, 1500),
    test("mooneye/emulator-only/mbc5/rom_512kb.gb", DMG, 1500),
    test("mooneye/emulator-only/mbc5/rom_64Mb.gb", DMG, 1500),
    test("mooneye/emulator-only/mbc5/rom_8Mb.gb", DMG, 1500),
    test("mooneye/manual-only/sprite_priority.gb", DMG, 1500),
    test("mooneye/misc/boot_div-cgbABCDE.gb", CGB, 1500),
    test("mooneye/misc/boot_regs-cgb.gb", CGB, 1500),
    test("samesuite/apu/channel_1/channel_1_align.gb", CGB, 1500),
    test("samesuite/apu/channel_1/channel_1_align_cpu.gb", CGB, 1500),
    test("samesuite/apu/channel_1/channel_1_delay.gb", CGB, 1500),
    test("samesuite/apu/channel_1/channel_1_duty.gb", CGB, 1500),
    test("samesuite/apu/channel_1/channel_1_duty_delay.gb", CGB, 1500),
    test(
        "samesuite/apu/channel_1/channel_1_freq_change.gb",
        CGB,
        1500,
    ),
    test(
        "samesuite/apu/channel_1/channel_1_nrx2_glitch.gb",
        CGB,
        1500,
    ),
    test(
        "samesuite/apu/channel_1/channel_1_nrx2_speed_change.gb",
        CGB,
        3500,
    ),
    test("samesuite/apu/channel_1/channel_1_restart.gb", CGB, 1500),
    test(
        "samesuite/apu/channel_1/channel_1_restart_nrx2_glitch.gb",
        CGB,
        1500,
    ),
    test("samesuite/apu/channel_1/channel_1_stop_div.gb", CGB, 1500),
    test(
        "samesuite/apu/channel_1/channel_1_stop_restart.gb",
        CGB,
        1500,
    ),
    test("samesuite/apu/channel_1/channel_1_sweep.gb", CGB, 1500),
    test(
        "samesuite/apu/channel_1/channel_1_sweep_restart.gb",
        CGB,
        1500,
    ),
    test(
        "samesuite/apu/channel_1/channel_1_sweep_restart_2.gb",
        CGB,
        1500,
    ),
    test("samesuite/apu/channel_1/channel_1_volume.gb", CGB, 1500),
    test("samesuite/apu/channel_1/channel_1_volume_div.gb", CGB, 2500),
    test("samesuite/apu/channel_2/channel_2_align.gb", CGB, 1500),
    test("samesuite/apu/channel_2/channel_2_align_cpu.gb", CGB, 1500),
    test("samesuite/apu/channel_2/channel_2_delay.gb", CGB, 1500),
    test("samesuite/apu/channel_2/channel_2_duty.gb", CGB, 1500),
    test("samesuite/apu/channel_2/channel_2_duty_delay.gb", CGB, 1500),
    test(
        "samesuite/apu/channel_2/channel_2_freq_change.gb",
        CGB,
        1500,
    ),
    test(
        "samesuite/apu/channel_2/channel_2_nrx2_glitch.gb",
        CGB,
        1500,
    ),
    test(
        "samesuite/apu/channel_2/channel_2_nrx2_speed_change.gb",
        CGB,
        3500,
    ),
    test("samesuite/apu/channel_2/channel_2_restart.gb", CGB, 1500),
    test(
        "samesuite/apu/channel_2/channel_2_restart_nrx2_glitch.gb",
        CGB,
        1500,
    ),
    test("samesuite/apu/channel_2/channel_2_stop_div.gb", CGB, 1500),
    test(
        "samesuite/apu/channel_2/channel_2_stop_restart.gb",
        CGB,
        1500,
    ),
    test("samesuite/apu/channel_2/channel_2_volume.gb", CGB, 1500),
    test("samesuite/apu/channel_2/channel_2_volume_div.gb", CGB, 1500),
    test("samesuite/apu/channel_3/channel_3_and_glitch.gb", CGB, 1500),
    test("samesuite/apu/channel_3/channel_3_delay.gb", CGB, 1500),
    test(
        "samesuite/apu/channel_3/channel_3_first_sample.gb",
        CGB,
        1500,
    ),
    test(
        "samesuite/apu/channel_3/channel_3_freq_change_delay.gb",
        CGB,
        1500,
    ),
    test(
        "samesuite/apu/channel_3/channel_3_restart_delay.gb",
        CGB,
        1500,
    ),
    test(
        "samesuite/apu/channel_3/channel_3_restart_during_delay.gb",
        CGB,
        1500,
    ),
    test(
        "samesuite/apu/channel_3/channel_3_restart_stop_delay.gb",
        CGB,
        1500,
    ),
    test(
        "samesuite/apu/channel_3/channel_3_shift_delay.gb",
        CGB,
        1500,
    ),
    test(
        "samesuite/apu/channel_3/channel_3_shift_skip_delay.gb",
        CGB,
        1500,
    ),
    test("samesuite/apu/channel_3/channel_3_stop_delay.gb", CGB, 1500),
    test("samesuite/apu/channel_3/channel_3_stop_div.gb", CGB, 1500),
    test(
        "samesuite/apu/channel_3/channel_3_wave_ram_locked_write.gb",
        CGB,
        1500,
    ),
    test(
        "samesuite/apu/channel_3/channel_3_wave_ram_sync.gb",
        CGB,
        1500,
    ),
    test("samesuite/apu/channel_4/channel_4_align.gb", CGB, 1500),
    test("samesuite/apu/channel_4/channel_4_delay.gb", CGB, 1500),
    test(
        "samesuite/apu/channel_4/channel_4_equivalent_frequencies.gb",
        CGB,
        1500,
    ),
    test(
        "samesuite/apu/channel_4/channel_4_frequency_alignment.gb",
        CGB,
        1500,
    ),
    test(
        "samesuite/apu/channel_4/channel_4_freq_change.gb",
        CGB,
        1500,
    ),
    test("samesuite/apu/channel_4/channel_4_lfsr.gb", CGB, 1500),
    test("samesuite/apu/channel_4/channel_4_lfsr15.gb", CGB, 1500),
    test("samesuite/apu/channel_4/channel_4_lfsr_15_7.gb", CGB, 1500),
    test("samesuite/apu/channel_4/channel_4_lfsr_7_15.gb", CGB, 1500),
    test(
        "samesuite/apu/channel_4/channel_4_lfsr_restart.gb",
        CGB,
        1500,
    ),
    test(
        "samesuite/apu/channel_4/channel_4_lfsr_restart_fast.gb",
        CGB,
        1500,
    ),
    test("samesuite/apu/channel_4/channel_4_volume_div.gb", CGB, 2500),
    test("samesuite/apu/div_trigger_volume_10.gb", CGB, 1500),
    test("samesuite/apu/div_write_trigger.gb", CGB, 1500),
    test("samesuite/apu/div_write_trigger_10.gb", CGB, 1500),
    test("samesuite/apu/div_write_trigger_volume.gb", CGB, 1500),
    test("samesuite/apu/div_write_trigger_volume_10.gb", CGB, 1500),
    test("samesuite/dma/gbc_dma_cont.gb", CGB, 1500),
    test("samesuite/dma/gdma_addr_mask.gb", CGB, 1500),
    test("samesuite/dma/hdma_lcd_off.gb", CGB, 1500),
    test("samesuite/dma/hdma_mode0.gb", CGB, 1500),
    test("samesuite/ppu/blocking_bgpi_increase.gb", CGB, 1500),
    test("ashiepaws/bully.gb (DMG)", DMG, 500).rom("ashiepaws/bully.gb"),
    test("ashiepaws/bully.gb (GBC)", CGB, 500).rom("ashiepaws/bully.gb"),
    test("ashiepaws/strikethrough.gb", DMG, 500).in_collection("strikethrough/strikethrough.gb"),
    test("cpp/rtc-invalid-banks-test.gb", DMG, 500),
    test("cpp/latch-rtc-test.gb", DMG, 500),
    test("cpp/ramg-mbc3-test.gb", DMG, 500),
    test(
        "mealybug-tearoom-tests/ppu/m2_win_en_toggle.gb (DMG)",
        DMG,
        500,
    )
    .rom("mealybug-tearoom-tests/ppu/m2_win_en_toggle.gb")
    .in_collection("mealybug-tearoom-tests/ppu/m2_win_en_toggle.gb")
    .results(&["mealybug-tearoom-tests/ppu/m2_win_en_toggle_dmg_blob.png"]),
    test(
        "mealybug-tearoom-tests/ppu/m3_bgp_change.gb (DMG)",
        DMG,
        500,
    )
    .rom("mealybug-tearoom-tests/ppu/m3_bgp_change.gb")
    .in_collection("mealybug-tearoom-tests/ppu/m3_bgp_change.gb")
    .results(&["mealybug-tearoom-tests/ppu/m3_bgp_change_dmg_blob.png"]),
    test(
        "mealybug-tearoom-tests/ppu/m3_bgp_change_sprites.gb (DMG)",
        DMG,
        500,
    )
    .rom("mealybug-tearoom-tests/ppu/m3_bgp_change_sprites.gb")
    .in_collection("mealybug-tearoom-tests/ppu/m3_bgp_change_sprites.gb")
    .results(&["mealybug-tearoom-tests/ppu/m3_bgp_change_sprites_dmg_blob.png"]),
    test(
        "mealybug-tearoom-tests/ppu/m3_lcdc_bg_en_change.gb (DMG)",
        DMG,
        500,
    )
    .rom("mealybug-tearoom-tests/ppu/m3_lcdc_bg_en_change.gb")
    .in_collection("mealybug-tearoom-tests/ppu/m3_lcdc_bg_en_change.gb")
    .results(&["mealybug-tearoom-tests/ppu/m3_lcdc_bg_en_change_dmg_blob.png"]),
    test(
        "mealybug-tearoom-tests/ppu/m3_lcdc_bg_map_change.gb (DMG)",
        DMG,
        500,
    )
    .rom("mealybug-tearoom-tests/ppu/m3_lcdc_bg_map_change.gb")
    .in_collection("mealybug-tearoom-tests/ppu/m3_lcdc_bg_map_change.gb")
    .results(&["mealybug-tearoom-tests/ppu/m3_lcdc_bg_map_change_dmg_blob.png"]),
    test(
        "mealybug-tearoom-tests/ppu/m3_lcdc_obj_en_change.gb (DMG)",
        DMG,
        500,
    )
    .rom("mealybug-tearoom-tests/ppu/m3_lcdc_obj_en_change.gb")
    .in_collection("mealybug-tearoom-tests/ppu/m3_lcdc_obj_en_change.gb")
    .results(&["mealybug-tearoom-tests/ppu/m3_lcdc_obj_en_change_dmg_blob.png"]),
    test(
        "mealybug-tearoom-tests/ppu/m3_lcdc_obj_en_change_variant.gb (DMG)",
        DMG,
        500,
    )
    .rom("mealybug-tearoom-tests/ppu/m3_lcdc_obj_en_change_variant.gb")
    .in_collection("mealybug-tearoom-tests/ppu/m3_lcdc_obj_en_change_variant.gb")
    .results(&["mealybug-tearoom-tests/ppu/m3_lcdc_obj_en_change_variant_dmg_blob.png"]),
    test(
        "mealybug-tearoom-tests/ppu/m3_lcdc_obj_size_change.gb (DMG)",
        DMG,
        500,
    )
    .rom("mealybug-tearoom-tests/ppu/m3_lcdc_obj_size_change.gb")
    .in_collection("mealybug-tearoom-tests/ppu/m3_lcdc_obj_size_change.gb")
    .results(&["mealybug-tearoom-tests/ppu/m3_lcdc_obj_size_change_dmg_blob.png"]),
    test(
        "mealybug-tearoom-tests/ppu/m3_lcdc_obj_size_change_scx.gb (DMG)",
        DMG,
        500,
    )
    .rom("mealybug-tearoom-tests/ppu/m3_lcdc_obj_size_change_scx.gb")
    .in_collection("mealybug-tearoom-tests/ppu/m3_lcdc_obj_size_change_scx.gb")
    .results(&["mealybug-tearoom-tests/ppu/m3_lcdc_obj_size_change_scx_dmg_blob.png"]),
    test(
        "mealybug-tearoom-tests/ppu/m3_lcdc_tile_sel_change.gb (DMG)",
        DMG,
        500,
    )
    .rom("mealybug-tearoom-tests/ppu/m3_lcdc_tile_sel_change.gb")
    .in_collection("mealybug-tearoom-tests/ppu/m3_lcdc_tile_sel_change.gb")
    .results(&["mealybug-tearoom-tests/ppu/m3_lcdc_tile_sel_change_dmg_blob.png"]),
    test(
        "mealybug-tearoom-tests/ppu/m3_lcdc_tile_sel_win_change.gb (DMG)",
        DMG,
        500,
    )
    .rom("mealybug-tearoom-tests/ppu/m3_lcdc_tile_sel_win_change.gb")
    .in_collection("mealybug-tearoom-tests/ppu/m3_lcdc_tile_sel_win_change.gb")
    .results(&["mealybug-tearoom-tests/ppu/m3_lcdc_tile_sel_win_change_dmg_blob.png"]),
    test(
        "mealybug-tearoom-tests/ppu/m3_lcdc_win_en_change_multiple.gb (DMG)",
        DMG,
        500,
    )
    .rom("mealybug-tearoom-tests/ppu/m3_lcdc_win_en_change_multiple.gb")
    .in_collection("mealybug-tearoom-tests/ppu/m3_lcdc_win_en_change_multiple.gb")
    .results(&["mealybug-tearoom-tests/ppu/m3_lcdc_win_en_change_multiple_dmg_blob.png"]),
    test(
        "mealybug-tearoom-tests/ppu/m3_lcdc_win_en_change_multiple_wx.gb (DMG)",
        DMG,
        500,
    )
    .rom("mealybug-tearoom-tests/ppu/m3_lcdc_win_en_change_multiple_wx.gb")
    .in_collection("mealybug-tearoom-tests/ppu/m3_lcdc_win_en_change_multiple_wx.gb")
    .results(&["mealybug-tearoom-tests/ppu/m3_lcdc_win_en_change_multiple_wx_dmg_blob.png"]),
    test(
        "mealybug-tearoom-tests/ppu/m3_lcdc_win_map_change.gb (DMG)",
        DMG,
        500,
    )
    .rom("mealybug-tearoom-tests/ppu/m3_lcdc_win_map_change.gb")
    .in_collection("mealybug-tearoom-tests/ppu/m3_lcdc_win_map_change.gb")
    .results(&["mealybug-tearoom-tests/ppu/m3_lcdc_win_map_change_dmg_blob.png"]),
    test(
        "mealybug-tearoom-tests/ppu/m3_obp0_change.gb (DMG)",
        DMG,
        500,
    )
    .rom("mealybug-tearoom-tests/ppu/m3_obp0_change.gb")
    .in_collection("mealybug-tearoom-tests/ppu/m3_obp0_change.gb")
    .results(&["mealybug-tearoom-tests/ppu/m3_obp0_change_dmg_blob.png"]),
    test(
        "mealybug-tearoom-tests/ppu/m3_scx_high_5_bits.gb (DMG)",
        DMG,
        500,
    )
    .rom("mealybug-tearoom-tests/ppu/m3_scx_high_5_bits.gb")
    .in_collection("mealybug-tearoom-tests/ppu/m3_scx_high_5_bits.gb")
    .results(&["mealybug-tearoom-tests/ppu/m3_scx_high_5_bits_dmg_blob.png"]),
    test(
        "mealybug-tearoom-tests/ppu/m3_scx_low_3_bits.gb (DMG)",
        DMG,
        500,
    )
    .rom("mealybug-tearoom-tests/ppu/m3_scx_low_3_bits.gb")
    .in_collection("mealybug-tearoom-tests/ppu/m3_scx_low_3_bits.gb")
    .results(&["mealybug-tearoom-tests/ppu/m3_scx_low_3_bits_dmg_blob.png"]),
    test(
        "mealybug-tearoom-tests/ppu/m3_scy_change.gb (DMG)",
        DMG,
        500,
    )
    .rom("mealybug-tearoom-tests/ppu/m3_scy_change.gb")
    .in_collection("mealybug-tearoom-tests/ppu/m3_scy_change.gb")
    .results(&["mealybug-tearoom-tests/ppu/m3_scy_change_dmg_blob.png"]),
    test(
        "mealybug-tearoom-tests/ppu/m3_window_timing.gb (DMG)",
        DMG,
        500,
    )
    .rom("mealybug-tearoom-tests/ppu/m3_window_timing.gb")
    .in_collection("mealybug-tearoom-tests/ppu/m3_window_timing.gb")
    .results(&["mealybug-tearoom-tests/ppu/m3_window_timing_dmg_blob.png"]),
    test(
        "mealybug-tearoom-tests/ppu/m3_window_timing_wx_0.gb (DMG)",
        DMG,
        500,
    )
    .rom("mealybug-tearoom-tests/ppu/m3_window_timing_wx_0.gb")
    .in_collection("mealybug-tearoom-tests/ppu/m3_window_timing_wx_0.gb")
    .results(&["mealybug-tearoom-tests/ppu/m3_window_timing_wx_0_dmg_blob.png"]),
    test(
        "mealybug-tearoom-tests/ppu/m3_wx_4_change.gb (DMG)",
        DMG,
        500,
    )
    .rom("mealybug-tearoom-tests/ppu/m3_wx_4_change.gb")
    .in_collection("mealybug-tearoom-tests/ppu/m3_wx_4_change.gb")
    .results(&["mealybug-tearoom-tests/ppu/m3_wx_4_change_dmg_blob.png"]),
    test(
        "mealybug-tearoom-tests/ppu/m3_wx_4_change_sprites.gb (DMG)",
        DMG,
        500,
    )
    .rom("mealybug-tearoom-tests/ppu/m3_wx_4_change_sprites.gb")
    .in_collection("mealybug-tearoom-tests/ppu/m3_wx_4_change_sprites.gb")
    .results(&["mealybug-tearoom-tests/ppu/m3_wx_4_change_sprites_dmg_blob.png"]),
    test(
        "mealybug-tearoom-tests/ppu/m3_wx_5_change.gb (DMG)",
        DMG,
        500,
    )
    .rom("mealybug-tearoom-tests/ppu/m3_wx_5_change.gb")
    .in_collection("mealybug-tearoom-tests/ppu/m3_wx_5_change.gb")
    .results(&["mealybug-tearoom-tests/ppu/m3_wx_5_change_dmg_blob.png"]),
    test(
        "mealybug-tearoom-tests/ppu/m3_wx_6_change.gb (DMG)",
        DMG,
        500,
    )
    .rom("mealybug-tearoom-tests/ppu/m3_wx_6_change.gb")
    .in_collection("mealybug-tearoom-tests/ppu/m3_wx_6_change.gb")
    .results(&["mealybug-tearoom-tests/ppu/m3_wx_6_change_dmg_blob.png"]),
];

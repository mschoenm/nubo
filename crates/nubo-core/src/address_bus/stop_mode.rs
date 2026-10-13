/*
    What stands still in STOP mode follows SameBoy's sm83_cpu.c, timing.c and display.c
    (https://github.com/LIJI32/SameBoy, Expat License, see the notice in apu.rs).
*/

//! STOP mode, which STOP enters when no speed switch is armed and no button
//! is held. The CPU, the timers, OAM DMA, HBlank DMA and the serial port
//! stand still until a button press pulls a joypad line low. Monochrome
//! models stop the PPU and show a white screen; the CGB's PPU keeps running
//! but is cut off from whatever the CPU could access when STOP started, so
//! the screen turns black unless STOP came during mode 3
//! (`Gpu::enter_stop_mode`).

use crate::address_bus::AddressBus;
use crate::clock::dots_per_m_cycle;

impl AddressBus {
    /// Whether a held button pulls a joypad input line low.
    pub(crate) fn joypad_line_low(&self) -> bool {
        self.joypad.any_line_low()
    }

    /// Enters STOP mode, which resets DIV. OAM DMA and HBlank DMA pause like
    /// they do in HALT.
    pub(crate) fn enter_stop_mode(&mut self) {
        self.run_deferred_cycles_before_change();
        self.reset_divider(false);
        self.catch_up_ppu_before_change();
        self.gpu.enter_stop_mode();
        self.dma_halt_started();
        self.hdma_halt_started();
    }

    /// Leaves STOP mode. OAM DMA and HBlank DMA resume like they do after HALT.
    pub(crate) fn leave_stop_mode(&mut self) {
        self.catch_up_ppu_before_change();
        self.gpu.leave_stop_mode();
        self.hdma_halt_ended();
        self.dma_halt_ended();
    }

    /// Runs one M-cycle in STOP mode, in which only the PPU and the APU run.
    /// Nothing can look at the PPU until a button ends STOP mode, so unless a
    /// paused OAM DMA transfer still holds OAM, the PPU is left behind like in
    /// any other M-cycle (`advance_ppu`): only what it would deliver (a frame)
    /// makes it catch up, instead of stepping it dot by dot every M-cycle.
    pub(crate) fn sync_stopped(&mut self) {
        self.apu.begin_machine_cycle();
        let dots = dots_per_m_cycle(self.speed_switch.cgb_double_speed());
        self.count_elapsed_dots(dots);
        let dots = self.take_cycle_dots(dots, 0);
        if self.dma.is_idle() && self.gpu.oam_dma_access_settled() {
            self.advance_ppu(dots);
        } else {
            self.catch_up_ppu_before_change();
            self.gpu.set_oam_dma_access(self.ppu_oam_dma_access(), 0);
            self.gpu.step_dots(dots, &mut self.hdma);
        }
        self.apu.step_while_stopped();
        if super::EAGER {
            self.apu.catch_up();
        }
    }
}

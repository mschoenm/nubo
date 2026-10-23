/*
    The frequency sweep's timing and quirks are ported from SameBoy's apu.c
    (https://github.com/LIJI32/SameBoy, Expat License, see the notice in apu.rs).
*/

//! Channel 1's frequency sweep, controlled by NR10.
//!
//! Every few sweep steps of the frame sequencer (the pace in NR10 bits 4-6),
//! the sweep calculates a new period by adding the current period shifted right
//! by NR10 bits 0-2, or subtracting it when bit 3 is set. When an addition
//! overflows 11 bits, channel 1 turns off. On real hardware the calculation
//! takes a few cycles, and several timing quirks depend on that.

use crate::apu::{Apu, Channel, MAX_PERIOD, NR10};
use crate::model::Model;

/// The calculation countdown an NR10 write leaves behind on CGB-C and older in
/// double speed, indexed by the old countdown.
const DOUBLE_SPEED_RELOAD_CORRUPTION: [u8; 8] = [7, 7, 5, 7, 3, 3, 5, 7];
/// A trigger this many 2 MHz ticks or less before a frame sequencer step that clocks channel 1's
/// sweep doesn't let that step count towards the sweep period.
const DMG_SWEEP_CLOCK_MISSED_TICKS: u32 = 2;
const CGB_SWEEP_CLOCK_MISSED_TICKS: u32 = 4;

/// NR10 bits 0-2: how far the sweep shifts the period right to get the amount
/// it adds or subtracts. With a shift of 0 the period stays as it is.
const fn sweep_shift(nr10: u8) -> u8 {
    nr10 & 7
}

/// NR10 bit 3: the sweep subtracts from the period (the pitch falls) rather
/// than adding to it.
const fn sweep_subtracts(nr10: u8) -> bool {
    nr10 & 8 != 0
}

/// NR10 bits 4-6: the frame sequencer's sweep steps (128 per second) between
/// sweep calculations. 0 stops the sweep.
const fn sweep_pace(nr10: u8) -> u8 {
    (nr10 >> 4) & 7
}

impl Apu {
    /// Restarts channel 1's sweep when the channel is triggered.
    pub(super) fn trigger_sweep(&mut self, was_active: bool) {
        let nr10 = self.registers[NR10];
        let tick_parity = self.state.tick_parity;
        let state = &mut self.state;

        state.square_sweep_instant_calculation_done = false;
        state.sweep_shadow_period = 0;
        state.channel_1_completed_addend = 0;

        if sweep_shift(nr10) != 0 {
            state.square_sweep_calculate_countdown = sweep_shift(nr10);
            let late_phase =
                self.model <= Model::CgbC && (tick_parity ^ u8::from(!self.cgb_double_speed)) != 0;
            state.square_sweep_calculate_countdown_reload_timer = if late_phase { 3 } else { 2 };
            state.unshifted_sweep = false;
            if !was_active {
                state.square_sweep_calculate_countdown_reload_timer += 1;
            }
            state.sweep_period_addend = state.square1.period >> sweep_shift(nr10);
        } else {
            state.sweep_period_addend = 0;
        }

        let longer_hold = self.cgb_mode && self.model != Model::CgbD;
        state.channel_1_restart_hold = if longer_hold {
            4 - tick_parity
        } else {
            2 - tick_parity
        };
        state.sweep_pace_counter = 7 - sweep_pace(nr10);

        // A sweep clock that comes in the last ticks after the trigger doesn't count towards the
        // sweep's first period (measured against Gambatte's behavior; Gambatte's sound
        // ch1_init_reset_sweep_counter_timing tests).
        let missed_ticks = if self.cgb_mode {
            CGB_SWEEP_CLOCK_MISSED_TICKS
        } else {
            DMG_SWEEP_CLOCK_MISSED_TICKS
        };
        if self.next_step_is_imminent(3, missed_ticks) {
            self.state.sweep_pace_counter = self.state.sweep_pace_counter.wrapping_sub(1) & 7;
        }
    }

    /// Finishes a sweep calculation: keeps the result for the next sweep step
    /// and turns channel 1 off when an addition overflows 11 bits.
    pub(super) fn sweep_calculation_done(&mut self) {
        let nr10 = self.registers[NR10];
        let state = &mut self.state;
        if state.channel_1_restart_hold == 0 {
            state.sweep_shadow_period = state.square1.period;
        }
        if sweep_subtracts(nr10) {
            state.sweep_period_addend ^= MAX_PERIOD;
        }
        let overflowed = state.sweep_shadow_period + state.sweep_period_addend > MAX_PERIOD
            && !sweep_subtracts(nr10);
        state.channel_1_completed_addend = state.sweep_period_addend;
        if overflowed {
            self.silence(Channel::Square1);
        }
    }

    /// Once the sweep pace has elapsed (checked on each frame sequencer sweep
    /// step and after every NR10 write, as SameBoy does): applies the previous
    /// result to channel 1's period and starts the next calculation.
    pub(super) fn trigger_sweep_calculation(&mut self, during_div_write: bool) {
        if !self.sweep_pace_elapsed(self.state.sweep_pace_counter) {
            return;
        }
        let nr10 = self.registers[NR10];
        let state = &mut self.state;

        if sweep_shift(nr10) != 0 {
            let negate = u16::from(sweep_subtracts(nr10));
            state.square1.period =
                (state.sweep_period_addend + state.sweep_shadow_period + negate) & MAX_PERIOD;
        }
        if state.channel_1_restart_hold == 0 {
            state.sweep_period_addend = state.square1.period >> sweep_shift(nr10);
        }

        state.square_sweep_calculate_countdown = sweep_shift(nr10);
        state.square_sweep_calculate_countdown_reload_timer = 1 + state.tick_parity;
        if !self.cgb_double_speed && during_div_write {
            state.square_sweep_calculate_countdown_reload_timer = 1;
        }
        state.unshifted_sweep = sweep_shift(nr10) == 0;
        state.sweep_pace_counter = 7 - sweep_pace(nr10);
        if state.square_sweep_calculate_countdown == 0 {
            state.square_sweep_instant_calculation_done = true;
        }
    }

    /// Advances a running sweep calculation by `ticks` 2 MHz ticks. The sweep
    /// runs at 1 MHz, so `sweep_cycles` counts 1 MHz cycles.
    pub(super) fn run_sweep(&mut self, ticks: u16) {
        let mut sweep_cycles = ticks / 2;
        if ticks & 1 != 0 && self.state.tick_parity == 0 {
            sweep_cycles += 1;
        }

        let reload_timer = u16::from(self.state.square_sweep_calculate_countdown_reload_timer);
        if reload_timer > sweep_cycles {
            self.state.square_sweep_calculate_countdown_reload_timer =
                u8::try_from(reload_timer - sweep_cycles).expect("less than the reload timer");
            sweep_cycles = 0;
        } else {
            if self.state.square_sweep_calculate_countdown_reload_timer != 0
                && self.state.square_sweep_calculate_countdown == 0
                && self.state.square_sweep_instant_calculation_done
            {
                self.sweep_calculation_done();
            }
            self.state.square_sweep_instant_calculation_done = false;
            sweep_cycles -= reload_timer;
            self.state.square_sweep_calculate_countdown_reload_timer = 0;
        }

        let countdown = u16::from(self.state.square_sweep_calculate_countdown);
        if countdown != 0 && self.sweep_calculation_counts_down() {
            if countdown > sweep_cycles {
                self.state.square_sweep_calculate_countdown =
                    u8::try_from(countdown - sweep_cycles).expect("less than the countdown");
            } else {
                self.state.square_sweep_calculate_countdown = 0;
                self.sweep_calculation_done();
            }
        }
    }

    /// Whether `square_sweep_calculate_countdown` counts down towards the end
    /// of a calculation: with a shift above 0, or for a calculation that
    /// started with a shift of 0.
    fn sweep_calculation_counts_down(&self) -> bool {
        sweep_shift(self.registers[NR10]) != 0 || self.state.unshifted_sweep
    }

    /// Whether channel 1's sweep is in the middle of a calculation, which a
    /// run does before it runs the squares, so running several M-cycles in
    /// one go would differ from running them one at a time.
    pub(super) fn sweep_calculating(&self) -> bool {
        self.state.square_sweep_calculate_countdown_reload_timer != 0
            || (self.state.square_sweep_calculate_countdown != 0
                && self.sweep_calculation_counts_down())
    }

    /// Whether the sweep step that brings the pace counter to `pace_counter`
    /// starts a sweep calculation: when the pace has elapsed, unless a pace
    /// of 0 stops the sweep.
    pub(super) fn sweep_pace_elapsed(&self, pace_counter: u8) -> bool {
        sweep_pace(self.registers[NR10]) != 0 && pace_counter == 7
    }

    /// Models how an NR10 write disturbs a sweep calculation in progress.
    fn nr10_write_glitch(&mut self, value: u8) {
        let nr10 = self.registers[NR10];
        let tick_parity = self.state.tick_parity;

        if self.model <= Model::CgbC {
            let reload_timer = self.state.square_sweep_calculate_countdown_reload_timer;
            let double_speed = self.cgb_double_speed;
            let countdown = self.state.square_sweep_calculate_countdown;
            if reload_timer == 1 && tick_parity == 0 {
                if double_speed {
                    self.state.square_sweep_calculate_countdown =
                        DOUBLE_SPEED_RELOAD_CORRUPTION[(countdown & 7) as usize];
                }
            } else if reload_timer > 1 {
                if double_speed {
                    self.state.square_sweep_calculate_countdown = sweep_shift(value);
                }
            } else if countdown != 0 {
                let should_zombie_step = if sweep_shift(nr10) == 0 {
                    (tick_parity ^ u8::from(double_speed)) != 0
                } else {
                    double_speed && countdown == 1
                };
                if should_zombie_step {
                    self.state.square_sweep_calculate_countdown -= 1;
                    if self.state.square_sweep_calculate_countdown <= 1 {
                        self.state.square_sweep_calculate_countdown = 0;
                        self.sweep_calculation_done();
                    }
                }
            }
            return;
        }

        if self.state.square_sweep_calculate_countdown_reload_timer == 2 {
            self.state.square_sweep_calculate_countdown = sweep_shift(value);
            if self.state.square_sweep_calculate_countdown == 0 {
                self.state.square_sweep_calculate_countdown_reload_timer = 0;
            }
        }
        if sweep_shift(value) != 0
            && sweep_shift(nr10) == 0
            && tick_parity == 0
            && self.state.square_sweep_calculate_countdown > 1
        {
            self.state.square_sweep_calculate_countdown -= 1;
            // Unreachable, since the countdown was above 1, but kept line for
            // line as in SameBoy's apu.c, where the same check is dead too.
            if self.state.square_sweep_calculate_countdown == 0 {
                self.sweep_calculation_done();
            }
        }
    }

    /// NR10 sets the sweep pace, direction and shift. Clearing the subtract bit
    /// turns channel 1 off at once when the last result would overflow.
    pub(super) fn write_nr10(&mut self, value: u8) {
        if self.state.square_sweep_calculate_countdown != 0
            || self.state.square_sweep_calculate_countdown_reload_timer != 0
        {
            self.nr10_write_glitch(value);
        }

        let old_negate = self.model <= Model::CgbC || sweep_subtracts(self.registers[NR10]);
        self.registers[NR10] = value;

        let state = &self.state;
        if state.sweep_shadow_period + state.channel_1_completed_addend + u16::from(old_negate)
            > MAX_PERIOD
            && !sweep_subtracts(value)
        {
            self.silence(Channel::Square1);
        }
        self.trigger_sweep_calculation(false);
    }
}

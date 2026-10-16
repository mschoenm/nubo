/*
    The timing of the switch (the delay, the freeze and the halt that follow STOP) is ported from
    SameBoy's sm83_cpu.c and timing.c (https://github.com/LIJI32/SameBoy, Expat License, see the
    notice in apu.rs).
*/

//! CGB double speed mode. The Game Boy Color can run its CPU twice as fast.
//! A game arms the switch through KEY1 (0xFF4D) and then executes STOP, which
//! toggles between normal and double speed.
//!
//! Time here is counted in CPU clock units, four per M-cycle at either
//! speed. A unit is one PPU dot at normal speed and half a dot at double
//! speed.
//!
//! The switch follows SameBoy. DIV resets after STOP reads its second byte
//! (`AddressBus::reset_divider_for_speed_switch`), double speed starts six
//! units later, the PPU and OAM DMA freeze for a few units, and the CPU halts
//! for 0x20008 units. The freeze lengths, and TIMA's jump ahead at the fast
//! TAC speeds when DIV resets (`TimerRegisters::reset_divider_for_speed_switch`),
//! are fitted to Gambatte's and AGE's hardware tests instead.

use crate::address_bus::AddressBus;
use crate::clock::dots_per_m_cycle;
use crate::utils::is_bit_set;
use nubo_serializable_derive::Serializable;

/// The CPU speed, the state of the KEY1 register and a switch in progress.
/// The address bus only routes KEY1 here on a CGB.
#[derive(Debug, Clone, Default, Serializable)]
pub(crate) struct SpeedSwitch {
    /// Whether the CPU runs at double speed. This is where the speed lives:
    /// the PPU, the APU and the rest of the address bus read it from here,
    /// and the PPU and APU keep copies that only
    /// `AddressBus::set_cgb_double_speed` changes.
    cgb_double_speed: bool,
    /// Set through KEY1 bit 0: the next STOP instruction switches speed.
    armed: bool,
    /// Units until a switch to double speed takes effect, or 0.
    double_speed_delay: u8,
    /// Units during which the PPU and OAM DMA stand still after a switch.
    freeze: u8,
    /// Units the CPU stays halted after a switch, or 0.
    halt_units: u32,
    /// Set when `halt_units` runs out, until the CPU wakes up.
    halt_ended: bool,
    /// The timers skip the next M-cycle: STOP holds DIV in reset for a
    /// little longer when IME is off.
    timer_hold: bool,
}

const ARMED_BIT: u8 = 0;
/// KEY1's bits 1-6 don't exist and read as 1.
const KEY1_UNUSED_BITS: u8 = 0x7E;
/// Ticks of the CPU's clock per M-cycle, which the system counter also counts
/// (`clock::SYSTEM_COUNTER_TICKS_PER_M_CYCLE`).
const UNITS_PER_M_CYCLE: u8 = 4;
/// Units after STOP until a switch to double speed takes effect.
const DOUBLE_SPEED_DELAY: u8 = 6;
/// How long the CPU stays halted after a switch, in units.
const SPEED_SWITCH_HALT_UNITS: u32 = 0x2_0008;
/// Units the PPU and OAM DMA stand still after a switch to double speed that
/// halts the CPU, and after a switch back to normal speed. SameBoy freezes
/// for 5 units in both cases; these values match Gambatte's hardware tests
/// (the speedchange ly44_m3 and m2int_m3stat families) instead.
const FREEZE_UNITS_TO_DOUBLE_SPEED: u8 = 1;
const FREEZE_UNITS_TO_NORMAL_SPEED: u8 = 3;
/// Units the PPU and OAM DMA stand still after a switch to double speed that
/// doesn't halt the CPU, because an interrupt was pending (SameBoy).
const FREEZE_UNITS: u8 = 1;
const JOYPAD_ADDRESS: u16 = 0xFF00;
const JOYPAD_LINES: u8 = 0x0F;

impl SpeedSwitch {
    /// Reads KEY1: bit 7 is the current speed, bit 0 whether a switch is
    /// armed, and the bits in between read as 1.
    pub(super) fn key1(&self) -> u8 {
        let double_speed_bit = u8::from(self.cgb_double_speed);
        let armed_bit = u8::from(self.armed);
        KEY1_UNUSED_BITS | double_speed_bit << 7 | armed_bit
    }

    /// Writes KEY1. Only bit 0, which arms the switch, is writable.
    pub(super) fn set_key1(&mut self, value: u8) {
        self.armed = is_bit_set(value, ARMED_BIT);
    }

    /// Whether the CPU runs at double speed.
    pub(crate) fn cgb_double_speed(&self) -> bool {
        self.cgb_double_speed
    }

    /// Whether a loaded save state holds values a switch can reach, which the
    /// emulator never saves otherwise: a delay, freeze and halt no longer than
    /// a switch starts with, no switch at all on a console without double speed
    /// (`cgb` is false), and no delay or freeze left once double speed has
    /// begun, where a freeze of an odd number of units would leave half a dot.
    pub(super) fn is_consistent(&self, cgb: bool) -> bool {
        let within_limits = self.double_speed_delay <= DOUBLE_SPEED_DELAY
            && self.freeze <= FREEZE_UNITS_TO_NORMAL_SPEED
            && self.halt_units <= SPEED_SWITCH_HALT_UNITS;
        let switching = self.cgb_double_speed
            || self.double_speed_delay != 0
            || self.freeze != 0
            || self.halt_units != 0;
        let settled_in_double_speed =
            !self.cgb_double_speed || (self.double_speed_delay == 0 && self.freeze == 0);
        within_limits && (cgb || !switching) && settled_in_double_speed
    }

    /// Whether no switch is in progress, so M-cycles run as usual.
    pub(super) fn is_idle(&self) -> bool {
        self.double_speed_delay == 0
            && self.freeze == 0
            && self.halt_units == 0
            && !self.halt_ended
            && !self.timer_hold
    }
}

/// What the hardware other than the CPU and timers does in one M-cycle,
/// given the speed switch.
pub(super) struct SpeedSwitchCycle {
    /// PPU dots in this M-cycle.
    pub(super) dots: u8,
    /// The whole M-cycle is frozen, so OAM DMA doesn't advance either.
    pub(super) frozen: bool,
}

impl AddressBus {
    /// Whether STOP switches speed now: a switch is armed on a CGB and no
    /// joypad line is pulled low (which would end STOP right away instead).
    pub(crate) fn speed_switch_requested(&self) -> bool {
        self.cgb_mode
            && self.speed_switch.armed
            && self.peek(JOYPAD_ADDRESS) & JOYPAD_LINES == JOYPAD_LINES
    }

    /// Lets the timers skip the next M-cycle, which STOP does when IME is off.
    pub(crate) fn hold_timers_after_stop(&mut self) {
        self.speed_switch.timer_hold = true;
    }

    /// Switches speed after STOP. Leaving double speed takes effect at
    /// once, entering it six units later. Unless an interrupt is pending,
    /// the CPU then halts for 0x20008 units, which the CPU side handles.
    pub(crate) fn switch_speed(&mut self, interrupt_pending: bool) {
        self.speed_switch.armed = false;
        let leaving_double_speed = self.speed_switch.cgb_double_speed;
        if leaving_double_speed {
            self.set_cgb_double_speed(false);
        } else {
            self.speed_switch.double_speed_delay = DOUBLE_SPEED_DELAY;
            self.speed_switch.freeze = FREEZE_UNITS;
        }
        if !interrupt_pending {
            self.speed_switch.halt_units = SPEED_SWITCH_HALT_UNITS;
            self.speed_switch.freeze = if leaving_double_speed {
                FREEZE_UNITS_TO_NORMAL_SPEED
            } else {
                FREEZE_UNITS_TO_DOUBLE_SPEED
            };
        }
    }

    /// Returns true once when the halt after a speed switch has run out.
    pub(crate) fn take_speed_switch_halt_end(&mut self) -> bool {
        std::mem::take(&mut self.speed_switch.halt_ended)
    }

    /// Ends the halt after a speed switch early, as an interrupt does.
    pub(crate) fn cancel_speed_switch_halt(&mut self) {
        self.speed_switch.halt_units = 0;
        self.speed_switch.halt_ended = false;
    }

    /// Changes the CPU speed, and the copies of it the PPU and APU keep.
    pub(super) fn set_cgb_double_speed(&mut self, cgb_double_speed: bool) {
        self.catch_up_ppu_before_change();
        self.speed_switch.cgb_double_speed = cgb_double_speed;
        self.apu.set_cgb_double_speed(cgb_double_speed);
        self.gpu.set_cgb_double_speed(cgb_double_speed);
    }

    /// Takes the timers' skipped M-cycle, if STOP asked for one.
    pub(super) fn take_timer_hold(&mut self) -> bool {
        std::mem::take(&mut self.speed_switch.timer_hold)
    }

    /// Advances a switch in progress by one M-cycle: applies a pending
    /// switch to double speed when its delay runs out (possibly halfway
    /// through the M-cycle), counts down the halt, and works out how many
    /// PPU dots are left after the freeze.
    #[inline]
    pub(super) fn advance_speed_switch(&mut self) -> SpeedSwitchCycle {
        let speed = &self.speed_switch;
        if speed.double_speed_delay == 0 && speed.freeze == 0 && speed.halt_units == 0 {
            return SpeedSwitchCycle {
                dots: dots_per_m_cycle(speed.cgb_double_speed),
                frozen: false,
            };
        }
        self.advance_speed_switch_in_progress()
    }

    /// `advance_speed_switch` while a switch is in progress.
    #[inline(never)]
    fn advance_speed_switch_in_progress(&mut self) -> SpeedSwitchCycle {
        let speed = &mut self.speed_switch;
        let before = speed.cgb_double_speed;
        let delay = speed.double_speed_delay;
        let segments = match delay {
            0 => [(UNITS_PER_M_CYCLE, before), (0, before)],
            _ if delay > UNITS_PER_M_CYCLE => {
                speed.double_speed_delay -= UNITS_PER_M_CYCLE;
                [(UNITS_PER_M_CYCLE, before), (0, before)]
            }
            // SameBoy switches before an M-cycle that the delay ends exactly at.
            UNITS_PER_M_CYCLE => [(UNITS_PER_M_CYCLE, !before), (0, !before)],
            _ => [(delay, before), (UNITS_PER_M_CYCLE - delay, !before)],
        };
        if delay != 0 && delay <= UNITS_PER_M_CYCLE {
            self.speed_switch.double_speed_delay = 0;
            self.set_cgb_double_speed(!before);
        }
        self.finish_speed_switch_cycle(segments)
    }

    /// Counts down the halt and the freeze over `segments` (units and
    /// whether they run at double speed) and turns the rest into dots.
    fn finish_speed_switch_cycle(&mut self, segments: [(u8, bool); 2]) -> SpeedSwitchCycle {
        let speed = &mut self.speed_switch;
        if speed.halt_units > 0 {
            speed.halt_units = speed
                .halt_units
                .saturating_sub(u32::from(UNITS_PER_M_CYCLE));
            speed.halt_ended = speed.halt_units == 0;
        }

        // Count in 8 MHz ticks: two per unit at normal speed, one at double.
        let mut ticks = 0;
        for (units, double_speed) in segments {
            let frozen = speed.freeze.min(units);
            speed.freeze -= frozen;
            ticks += (units - frozen) * if double_speed { 1 } else { 2 };
        }
        let frozen = ticks == 0;
        // Only whole dots run: the switch to double speed takes effect two units into an M-cycle
        // after the freeze has ended, so an even number of units run at double speed. Freezes that
        // ended within double speed units could leave half a dot, which would have to be carried
        // over to the next M-cycle.
        debug_assert!(ticks % 2 == 0, "a speed switch left half a dot");
        SpeedSwitchCycle {
            dots: ticks / 2,
            frozen,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::address_bus::test_utils::initialize_test_address_bus;
    use crate::model::Model;

    /// An address bus of a CGB, the only model that can switch speed.
    fn cgb_address_bus() -> AddressBus {
        let mut address_bus = initialize_test_address_bus();
        address_bus.set_model(Model::CgbE);
        address_bus
    }

    #[test]
    fn should_read_from_key1_in_double_speed_mode() {
        let speed_switch = SpeedSwitch {
            cgb_double_speed: true,
            ..SpeedSwitch::default()
        };
        assert_eq!(speed_switch.key1(), 0xFE);
    }

    #[test]
    fn should_read_from_key1_while_speed_switch_is_armed() {
        let speed_switch = SpeedSwitch {
            armed: true,
            ..SpeedSwitch::default()
        };
        assert_eq!(speed_switch.key1(), 0x7F);
    }

    #[test]
    fn a_loaded_switch_is_consistent_only_where_a_switch_can_get_to() {
        let double_speed = SpeedSwitch {
            cgb_double_speed: true,
            ..SpeedSwitch::default()
        };
        assert!(double_speed.is_consistent(true));
        assert!(!double_speed.is_consistent(false));
        assert!(
            !SpeedSwitch {
                freeze: 1,
                ..double_speed.clone()
            }
            .is_consistent(true)
        );
        assert!(
            !SpeedSwitch {
                double_speed_delay: 2,
                ..double_speed
            }
            .is_consistent(true)
        );

        let entering = SpeedSwitch {
            double_speed_delay: DOUBLE_SPEED_DELAY,
            freeze: FREEZE_UNITS_TO_DOUBLE_SPEED,
            halt_units: SPEED_SWITCH_HALT_UNITS,
            ..SpeedSwitch::default()
        };
        assert!(entering.is_consistent(true));
        assert!(!entering.is_consistent(false));
    }

    #[test]
    fn should_write_to_key1() {
        let mut speed_switch = SpeedSwitch::default();
        speed_switch.set_key1(0x1);
        assert!(speed_switch.armed);
    }

    /// The dots of the next `count` M-cycles.
    fn dots(address_bus: &mut AddressBus, count: usize) -> Vec<u8> {
        (0..count)
            .map(|_| address_bus.advance_speed_switch().dots)
            .collect()
    }

    #[test]
    fn should_enter_double_speed_halfway_through_the_second_m_cycle() {
        let mut address_bus = cgb_address_bus();
        address_bus.switch_speed(false);
        // Frozen for 1 unit, 3 units at normal speed (3 dots), then 2 more at
        // normal speed and 2 at double speed (3 dots).
        assert_eq!(dots(&mut address_bus, 3), vec![3, 3, 2]);
        assert!(address_bus.speed_switch.cgb_double_speed());
        assert!(!address_bus.speed_switch.armed);
    }

    #[test]
    fn should_leave_double_speed_at_once() {
        let mut address_bus = cgb_address_bus();
        address_bus.set_cgb_double_speed(true);
        address_bus.switch_speed(false);
        assert!(!address_bus.speed_switch.cgb_double_speed());
        // Frozen for 3 units, then normal speed.
        assert_eq!(dots(&mut address_bus, 3), vec![1, 4, 4]);
    }

    #[test]
    fn should_switch_without_halting_when_an_interrupt_is_pending() {
        let mut address_bus = cgb_address_bus();
        address_bus.switch_speed(true);
        // Frozen for 1 unit, 5 units at normal speed, then double speed.
        assert_eq!(dots(&mut address_bus, 3), vec![3, 3, 2]);
        assert!(!address_bus.take_speed_switch_halt_end());
    }

    #[test]
    fn should_end_the_halt_after_0x20008_units() {
        let mut address_bus = cgb_address_bus();
        address_bus.switch_speed(false);
        for _ in 0..(SPEED_SWITCH_HALT_UNITS / 4 - 1) {
            address_bus.advance_speed_switch();
            assert!(!address_bus.take_speed_switch_halt_end());
        }
        address_bus.advance_speed_switch();
        assert!(address_bus.take_speed_switch_halt_end());
    }
}

//! How the Game Boy's clocks relate to each other, in one place.
//!
//! - The PPU draws one dot per tick of the 4 MHz main clock (4,194,304 Hz),
//!   whatever speed the CPU runs at.
//! - The CPU does everything in M-cycles (machine cycles) of four ticks of its
//!   own clock. That clock is the main clock at normal speed and twice as fast
//!   in the Game Boy Color's double speed mode, so an M-cycle lasts four dots
//!   at normal speed and two in double speed.
//! - The system counter, the 16-bit counter whose upper byte is DIV, counts
//!   the CPU's clock: four per M-cycle at either speed. The timer, the APU's
//!   frame sequencer and the serial clock are all driven by its bits.
//!
//! Older Game Boy documents call a tick a "T-cycle", which can mean either a
//! dot or a tick of the CPU's clock; the names here say which one they count.

/// How much the system counter counts up per M-cycle, at either speed.
pub(crate) const SYSTEM_COUNTER_TICKS_PER_M_CYCLE: u16 = 4;

/// PPU dots per M-cycle: four at normal speed, and two in double speed, where
/// the CPU runs twice as fast while the PPU keeps its pace.
pub(crate) fn dots_per_m_cycle(double_speed: bool) -> u8 {
    if double_speed { 2 } else { 4 }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_m_cycle_lasts_four_dots_at_normal_speed_and_two_in_double_speed() {
        assert_eq!(dots_per_m_cycle(false), 4);
        assert_eq!(dots_per_m_cycle(true), 2);
    }
}

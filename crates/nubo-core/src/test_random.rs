//! A pseudo-random number generator that tests share, so their random cases
//! are the same on every run.

/// A small pseudo-random number generator (xorshift64*), so the random
/// scenarios are the same on every run. Its seed must not be 0, which it
/// never leaves.
pub(crate) struct Random(pub(crate) u64);

impl Random {
    pub(crate) fn next(&mut self) -> u64 {
        self.0 ^= self.0 >> 12;
        self.0 ^= self.0 << 25;
        self.0 ^= self.0 >> 27;
        self.0.wrapping_mul(0x2545_F491_4F6C_DD1D)
    }

    /// A number below `limit`.
    pub(crate) fn below(&mut self, limit: usize) -> usize {
        (self.next() % limit as u64) as usize
    }

    pub(crate) fn byte(&mut self) -> u8 {
        self.next() as u8
    }
}

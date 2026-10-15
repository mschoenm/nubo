//! The game's recent past, for rewinding: a save state every few frames,
//! going back up to [`MAX_STATES`] states or [`MAX_BYTES`] of memory.
//!
//! States a few frames apart are mostly the same, so only the newest is
//! kept whole. Each older one is kept as its difference from the next newer
//! one (the bytes that differ, with runs of equal bytes skipped), and is
//! made whole again from it when rewinding reaches it.

use std::collections::VecDeque;

/// Every how many frames a state is kept: 15 a second.
pub const FRAMES_PER_STATE: u32 = 4;
/// The most states kept: 30 seconds of play.
const MAX_STATES: usize = 30 * 60 / FRAMES_PER_STATE as usize;
/// The most memory the history takes; the oldest states go first.
const MAX_BYTES: usize = 64 * 1024 * 1024;

/// An older state, kept as its difference from the next newer one, or whole
/// when the two differ in size.
#[derive(Debug)]
enum Older {
    Difference(Vec<u8>),
    Whole(Vec<u8>),
}

impl Older {
    fn len(&self) -> usize {
        match self {
            Older::Difference(bytes) | Older::Whole(bytes) => bytes.len(),
        }
    }
}

/// The states kept for rewinding, oldest first.
#[derive(Debug, Default)]
pub struct History {
    /// The newest state, whole.
    newest: Option<Vec<u8>>,
    /// The older states, oldest first, each relative to the next newer one.
    older: VecDeque<Older>,
    /// The bytes all the states take together.
    bytes: usize,
    /// Frames since the last state was kept.
    frames: u32,
}

impl History {
    /// Counts a finished frame, and says whether it is time to keep a state
    /// (see [`History::push`]).
    pub fn frame_finished(&mut self) -> bool {
        self.frames += 1;
        if self.frames >= FRAMES_PER_STATE {
            self.frames = 0;
            true
        } else {
            false
        }
    }

    /// Keeps `state` as the newest, dropping the oldest states once there
    /// are too many or they take too much memory.
    pub fn push(&mut self, state: Vec<u8>) {
        self.bytes += state.len();
        if let Some(previous) = self.newest.replace(state) {
            self.bytes -= previous.len();
            let newest = self.newest.as_deref().unwrap_or_default();
            let older = match difference(&previous, newest) {
                Some(difference) => Older::Difference(difference),
                None => Older::Whole(previous),
            };
            self.bytes += older.len();
            self.older.push_back(older);
        }

        while self.older.len() > MAX_STATES || (self.bytes > MAX_BYTES && !self.older.is_empty()) {
            if let Some(oldest) = self.older.pop_front() {
                self.bytes -= oldest.len();
            }
        }
    }

    /// Takes the newest state off the history, for rewinding to it; the one
    /// before it becomes the newest. `None` once rewinding has gone back as
    /// far as the history goes.
    pub fn pop(&mut self) -> Option<Vec<u8>> {
        let newest = self.newest.take()?;
        self.bytes -= newest.len();
        self.frames = 0;

        if let Some(older) = self.older.pop_back() {
            self.bytes -= older.len();
            let whole = match older {
                Older::Difference(difference) => apply(&difference, &newest),
                Older::Whole(whole) => whole,
            };
            self.bytes += whole.len();
            self.newest = Some(whole);
        }
        Some(newest)
    }

    /// How many states the history holds.
    #[cfg(test)]
    pub fn len(&self) -> usize {
        self.older.len() + usize::from(self.newest.is_some())
    }

    /// Forgets every state, such as when the game starts again.
    pub fn clear(&mut self) {
        *self = History::default();
    }
}

/// `older` as its difference from `newer`, the same length: runs of bytes,
/// each a 4-byte little-endian count of equal bytes skipped, a 4-byte count
/// of differing bytes, and those bytes of `older`. `None` if the lengths
/// differ.
fn difference(older: &[u8], newer: &[u8]) -> Option<Vec<u8>> {
    if older.len() != newer.len() {
        return None;
    }

    let mut out = Vec::new();
    let mut position = 0;
    while position < older.len() {
        let same = older[position..]
            .iter()
            .zip(&newer[position..])
            .take_while(|(old, new)| old == new)
            .count();
        position += same;
        let different = older[position..]
            .iter()
            .zip(&newer[position..])
            .take_while(|(old, new)| old != new)
            .count();
        out.extend_from_slice(&(same as u32).to_le_bytes());
        out.extend_from_slice(&(different as u32).to_le_bytes());
        out.extend_from_slice(&older[position..position + different]);
        position += different;
    }
    Some(out)
}

/// The older state that `difference` (see [`difference`]) and `newer` make.
fn apply(difference: &[u8], newer: &[u8]) -> Vec<u8> {
    let mut older = newer.to_vec();
    let mut position = 0;
    let mut read = 0;
    let count = |at: usize| -> usize {
        difference
            .get(at..at + 4)
            .and_then(|bytes| bytes.try_into().ok())
            .map_or(0, |bytes| u32::from_le_bytes(bytes) as usize)
    };
    while read + 8 <= difference.len() {
        position += count(read);
        let different = count(read + 4);
        read += 8;
        let Some(bytes) = difference.get(read..read + different) else {
            break;
        };
        if let Some(target) = older.get_mut(position..position + different) {
            target.copy_from_slice(bytes);
        }
        position += different;
        read += different;
    }
    older
}

#[cfg(test)]
mod tests;

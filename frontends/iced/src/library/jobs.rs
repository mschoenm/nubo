//! A queue of background jobs, one per game, of which only a few run at
//! the same time, such as capturing title screens or downloading box art.

use std::collections::{HashSet, VecDeque};
use std::path::{Path, PathBuf};

/// The games waiting for a job, and how many jobs are running. At most
/// `LIMIT` jobs run at the same time.
#[derive(Debug, Default)]
pub struct Jobs<const LIMIT: usize> {
    waiting: VecDeque<PathBuf>,
    /// Games waiting or running, so none gets two jobs.
    pending: HashSet<PathBuf>,
    running: usize,
}

impl<const LIMIT: usize> Jobs<LIMIT> {
    /// Queues the games in `paths` that aren't queued or running yet.
    pub fn add(&mut self, paths: impl IntoIterator<Item = PathBuf>) {
        for path in paths {
            if self.pending.insert(path.clone()) {
                self.waiting.push_back(path);
            }
        }
    }

    /// The games whose jobs can start now, counted as running.
    pub fn start(&mut self) -> Vec<PathBuf> {
        let free = LIMIT.saturating_sub(self.running);
        let started: Vec<PathBuf> = (0..free).map_while(|_| self.waiting.pop_front()).collect();
        self.running += started.len();
        started
    }

    /// Notes that the job for `path` finished.
    pub fn finish(&mut self, path: &Path) {
        self.running = self.running.saturating_sub(1);
        self.pending.remove(path);
    }

    /// Drops the waiting jobs, for example when downloads fail because
    /// there is no internet. Running jobs still finish.
    pub fn cancel_waiting(&mut self) {
        for path in self.waiting.drain(..) {
            self.pending.remove(&path);
        }
    }

    /// Whether nothing is waiting or running.
    pub fn is_idle(&self) -> bool {
        self.running == 0 && self.waiting.is_empty()
    }
}

//! Problems found away from the UI, such as a missing sound device or a save
//! that couldn't be written. They are collected here so the window can show
//! them as toasts, and printed on stderr as well.

use std::sync::{Arc, Mutex, PoisonError};

/// A shared list of problem messages waiting to be shown. Clones share the
/// same list, so the emulator thread can report and the UI can collect.
#[derive(Debug, Clone, Default)]
pub struct Notices {
    messages: Arc<Mutex<Vec<String>>>,
}

impl Notices {
    /// Adds a message for the window to show and prints it on stderr.
    pub fn report(&self, message: String) {
        eprintln!("{message}");
        self.messages
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .push(message);
    }

    /// Removes and returns the messages reported since the last call.
    pub fn take(&self) -> Vec<String> {
        std::mem::take(&mut *self.messages.lock().unwrap_or_else(PoisonError::into_inner))
    }
}

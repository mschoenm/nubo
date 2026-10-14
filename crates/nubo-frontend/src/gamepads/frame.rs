//! The browser build: the controllers are read on every frame, on the one
//! thread the page has.

use super::{ControllerEvent, Reader};
use std::cell::RefCell;
use std::collections::VecDeque;

/// The reader, and the events it found that still have to be reported.
/// The app takes one event per frame.
struct State {
    reader: Option<Reader>,
    pending: VecDeque<ControllerEvent>,
    started: bool,
}

thread_local! {
    static STATE: RefCell<State> = const {
        RefCell::new(State {
            reader: None,
            pending: VecDeque::new(),
            started: false,
        })
    };
}

/// Reads the controllers and returns the oldest event not reported yet, if
/// any. The app calls it once per frame. The first call starts reading them.
pub fn next_event() -> Option<ControllerEvent> {
    STATE.with_borrow_mut(|state| {
        if !state.started {
            state.started = true;
            match Reader::open() {
                Ok(reader) => {
                    state.pending.extend(reader.connected());
                    state.reader = Some(reader);
                }
                Err(error) => state.pending.push_back(ControllerEvent::Unavailable(error)),
            }
        }

        if let Some(reader) = &mut state.reader {
            let events = reader.poll();
            state.pending.extend(events);
        }

        state.pending.pop_front()
    })
}

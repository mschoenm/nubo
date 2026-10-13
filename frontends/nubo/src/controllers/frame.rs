//! The browser build: the controllers are read on every frame, on the one
//! thread the page has.

use super::{ControllerEvent, Reader};
use iced::{Subscription, window};
use std::cell::RefCell;
use std::collections::VecDeque;

/// The reader, and the events it found that still have to be reported.
/// A subscription can only report one event per frame.
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

pub fn subscription() -> Subscription<ControllerEvent> {
    window::frames().filter_map(|_| next_event())
}

fn next_event() -> Option<ControllerEvent> {
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
